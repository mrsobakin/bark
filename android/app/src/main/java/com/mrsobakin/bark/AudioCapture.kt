package com.mrsobakin.bark

import android.content.Context
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaCodec
import android.media.MediaCodecList
import android.media.MediaFormat
import android.media.MediaRecorder
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.sqrt
import java.io.ByteArrayOutputStream

/**
 * Captures audio via [AudioRecord] and produces a 16 kHz 16-bit mono
 * OGG/Opus buffer with **realtime** encoding.
 *
 * Unlike the old approach (record all PCM, then encode at the end), this
 * feeds PCM frames through the Opus encoder as they arrive from VAD so the
 * CPU-intensive encoding step is pipelined with capture.  No temporary files
 * are used – the OGG container is written entirely in memory via [OggOpusWriter].
 */
class AudioCapture {

    companion object {
        private const val TAG = "BarkAudio"
        private const val SAMPLE_RATE = 16000

        /** VAD operates on 512-sample (1024-byte) frames. */
        private const val VAD_FRAME_SHORTS = 512

        /**
         * Encoder frame: 20 ms of 16-bit mono PCM at 16 kHz.
         * 320 samples × 2 bytes = 640 bytes.
         */
        private const val ENCODER_FRAME_BYTES = 640
    }

    @Volatile
    private var active = false

    val isActive: Boolean get() = active

    /** Cached VAD to avoid reloading the Silero model between recordings. */
    private var cachedVad: VADProcessor? = null

    // Public API ----------------------------------------------------------

    /**
     * Record audio and return an OGG/Opus [ByteArray] ready for upload.
     *
     * @param maxDurationSec  Hard time limit (default 300 s = 5 min).
     * @param context         Android context (needed for Silero VAD model).
     * @param onLevel         Called on IO thread for each processed frame with RMS level in 0..1.
     * @return The OGG/Opus bytes, or `null` if no audio was captured.
     */
    suspend fun recordOnce(
        maxDurationSec: Int = 300,
        context: Context,
        onLevel: ((Float) -> Unit)? = null,
    ): ByteArray? = withContext(Dispatchers.IO) {
        val bufferSize = AudioRecord.getMinBufferSize(
            SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
        )

        val record = AudioRecord(
            MediaRecorder.AudioSource.VOICE_RECOGNITION,
            SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
            bufferSize,
        )

        record.startRecording()
        active = true

        val vad = cachedVad?.also { it.reset() } ?: VADProcessor(context).also { cachedVad = it }

        // Start the Opus encoder and prepare the in-memory OGG writer.
        val codec = createOpusEncoder()
        if (codec == null) {
            Log.d(TAG, "Opus encoder not available")
            active = false
            record.stop()
            record.release()
            return@withContext null
        }
        val oggWriter = OggOpusWriter()
        val bufferInfo = MediaCodec.BufferInfo()

        try {
            val frameBytes = VAD_FRAME_SHORTS * 2  // 1024 bytes
            val buf = ByteArray(frameBytes)
            val deadlineMs = System.currentTimeMillis() + maxDurationSec * 1000L
            val pcmBuffer = ByteArrayOutputStream()

            var encoderInputOffset = 0
            var presentationTimeUs = 0L
            var headersWritten = false

            // ── Main capture + encode loop ──────────────────────────
            outer@ while (active && System.currentTimeMillis() < deadlineMs) {
                val nRead = record.read(buf, 0, buf.size)
                if (nRead <= 0) continue

                onLevel?.invoke(computeRmsLevel(buf, nRead))
                vad.process(buf, pcmBuffer)

                // Feed newly available PCM to the encoder in fixed-size chunks.
                val pcm = pcmBuffer.toByteArray()
                while (encoderInputOffset + ENCODER_FRAME_BYTES <= pcm.size) {
                    // -- Feed one chunk --
                    val inputIndex = codec.dequeueInputBuffer(10_000)
                    if (inputIndex >= 0) {
                        val inBuf = codec.getInputBuffer(inputIndex)!!
                        inBuf.clear()
                        inBuf.put(pcm, encoderInputOffset, ENCODER_FRAME_BYTES)
                        codec.queueInputBuffer(
                            inputIndex, 0, ENCODER_FRAME_BYTES,
                            presentationTimeUs, 0,
                        )
                    }
                    encoderInputOffset += ENCODER_FRAME_BYTES
                    presentationTimeUs +=
                        (ENCODER_FRAME_BYTES / 2) * 1_000_000L / SAMPLE_RATE

                    // -- Drain all available encoder output --
                    drain@ while (true) {
                        val outIndex = codec.dequeueOutputBuffer(bufferInfo, 0)
                        when {
                            outIndex >= 0 -> {
                                val isConfig =
                                    (bufferInfo.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG) != 0
                                if (isConfig && bufferInfo.size > 0 && !headersWritten) {
                                    val csd = readBuffer(codec, outIndex, bufferInfo)
                                    oggWriter.writeOpusHead(csd)
                                    oggWriter.writeOpusTags()
                                    headersWritten = true
                                } else if (bufferInfo.size > 0) {
                                    val packet = readBuffer(codec, outIndex, bufferInfo)
                                    oggWriter.writeAudioPacket(
                                        packet, ENCODER_FRAME_BYTES / 2,
                                    )
                                }
                                codec.releaseOutputBuffer(outIndex, false)

                                if (bufferInfo.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) {
                                    break@outer
                                }
                            }

                            outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                                if (!headersWritten) {
                                    val fmt = codec.outputFormat
                                    val csdBuf = fmt.getByteBuffer("csd-0")
                                    if (csdBuf != null) {
                                        val csd = ByteArray(csdBuf.remaining()).also { csdBuf.get(it) }
                                        oggWriter.writeOpusHead(csd)
                                        oggWriter.writeOpusTags()
                                        headersWritten = true
                                    }
                                }
                            }

                            else -> break@drain  // INFO_TRY_AGAIN_LATER
                        }
                    }
                }
            }

            // ── Flush encoder ────────────────────────────────────────
            val flushIdx = codec.dequeueInputBuffer(10_000)
            if (flushIdx >= 0) {
                codec.queueInputBuffer(
                    flushIdx, 0, 0, presentationTimeUs,
                    MediaCodec.BUFFER_FLAG_END_OF_STREAM,
                )
            }

            // Drain remaining output (with a timeout to avoid infinite loop).
            var stallCount = 0
            while (stallCount < 10) {
                val outIndex = codec.dequeueOutputBuffer(bufferInfo, 5_000)
                when {
                    outIndex >= 0 -> {
                        stallCount = 0
                        val isConfig =
                            (bufferInfo.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG) != 0
                        if (isConfig && bufferInfo.size > 0 && !headersWritten) {
                            val csd = readBuffer(codec, outIndex, bufferInfo)
                            oggWriter.writeOpusHead(csd)
                            oggWriter.writeOpusTags()
                            headersWritten = true
                        } else if (bufferInfo.size > 0) {
                            val packet = readBuffer(codec, outIndex, bufferInfo)
                            oggWriter.writeAudioPacket(
                                packet, ENCODER_FRAME_BYTES / 2,
                            )
                        }
                        codec.releaseOutputBuffer(outIndex, false)

                        if (bufferInfo.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) break
                    }

                    outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                        if (!headersWritten) {
                            val fmt = codec.outputFormat
                            val csdBuf = fmt.getByteBuffer("csd-0")
                            if (csdBuf != null) {
                                val csd = ByteArray(csdBuf.remaining()).also { csdBuf.get(it) }
                                oggWriter.writeOpusHead(csd)
                                oggWriter.writeOpusTags()
                                headersWritten = true
                            }
                        }
                    }

                    else -> stallCount++  // INFO_TRY_AGAIN_LATER
                }
            }

            // ── Finalise OGG container ──────────────────────────────
            if (!headersWritten) {
                // Shouldn't happen with a standard Opus encoder, but be safe.
                Log.w(TAG, "encoder did not produce CSD-0, constructing default OpusHead")
                writeDefaultOpusHead(oggWriter)
            }
            oggWriter.close()

            val result = oggWriter.toByteArray()
            return@withContext if (result.isEmpty()) null else result
        } finally {
            active = false
            runCatching { record.stop() }
            record.release()
            runCatching { codec.stop() }
            codec.release()
        }
    }

    fun cancel() {
        active = false
    }

    fun cleanup() {
        cachedVad?.close()
        cachedVad = null
    }

    // Private helpers ---------------------------------------------------

    /** Create and start a MediaCodec Opus encoder, or return null. */
    private fun createOpusEncoder(): MediaCodec? {
        val format = MediaFormat.createAudioFormat(
            MediaFormat.MIMETYPE_AUDIO_OPUS, SAMPLE_RATE, 1,
        ).apply {
            setInteger(MediaFormat.KEY_BIT_RATE, 24_000)
            setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, ENCODER_FRAME_BYTES)
        }

        val encoderName = MediaCodecList(MediaCodecList.REGULAR_CODECS)
            .findEncoderForFormat(format) ?: return null

        return MediaCodec.createByCodecName(encoderName).apply {
            configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            start()
        }
    }

    /** Extract [info.size] bytes from an output buffer and return as [ByteArray]. */
    private fun readBuffer(
        codec: MediaCodec,
        index: Int,
        info: MediaCodec.BufferInfo,
    ): ByteArray {
        val buf = codec.getOutputBuffer(index)!!
        buf.position(info.offset)
        buf.limit(info.offset + info.size)
        val data = ByteArray(info.size)
        buf.get(data)
        return data
    }

    // ---- RMS amplitude (for level indicator) -------------------------

    /** Construct a default OpusHead for the (unlikely) case the encoder doesn't produce CSD-0. */
    private fun writeDefaultOpusHead(oggWriter: OggOpusWriter) {
        val csd = ByteArray(19).apply {
            "OpusHead".encodeToByteArray().copyInto(this, 0)
            this[8] = 1   // version
            this[9] = 1   // channels (mono)
            // pre-skip = 312 (LE16) — standard Opus lookahead
            this[10] = (312 and 0xFF).toByte()
            this[11] = ((312 shr 8) and 0xFF).toByte()
            // input sample rate = 16000 (LE32)
            this[12] = (16000 and 0xFF).toByte()
            this[13] = ((16000 shr 8) and 0xFF).toByte()
            this[14] = ((16000 shr 16) and 0xFF).toByte()
            this[15] = ((16000 shr 24) and 0xFF).toByte()
            // output gain = 0 (LE16)
            this[16] = 0; this[17] = 0
            // channel mapping family = 0 (mono)
            this[18] = 0
        }
        oggWriter.writeOpusHead(csd)
        oggWriter.writeOpusTags()
    }

    /** Normalised RMS (0..1) for a 16-bit PCM frame. */
    private fun computeRmsLevel(buffer: ByteArray, bytesRead: Int): Float {
        val samples = bytesRead / 2
        var sumSquares = 0.0
        var i = 0
        while (i < samples) {
            val lo = buffer[i * 2].toInt() and 0xFF
            val hi = buffer[i * 2 + 1].toInt() shl 8
            val sample = lo or hi
            sumSquares += (sample * sample).toDouble()
            i++
        }
        val rms = sqrt(sumSquares / samples)
        return (rms / 32768.0).toFloat().coerceIn(0f, 1f)
    }
}
