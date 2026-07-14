package com.mrsobakin.bark

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.SystemClock
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import java.io.IOException
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.sqrt

class AudioCapture {

    companion object {
        private const val SAMPLE_RATE = 16000
        private const val BUF_SAMPLES = 512
    }

    @Volatile
    private var active = false

    val isActive: Boolean get() = active

    private val generation = AtomicLong()
    private var cachedPipeline: BarkPipeline? = null
    private var cachedConfigJson: String? = null

    @SuppressLint("MissingPermission")
    suspend fun recordOnce(
        maxDurationSec: Int = 300,
        configJson: String,
        onLevel: ((Float) -> Unit)? = null,
        onTranscribing: (() -> Unit)? = null,
    ): String? {
        val session = generation.incrementAndGet()

        return withContext(Dispatchers.IO) {
            var record: AudioRecord? = null

            try {
                currentCoroutineContext().ensureActive()

                val bufferSize = AudioRecord.getMinBufferSize(
                    SAMPLE_RATE,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT,
                )
                if (bufferSize <= 0) {
                    throw IOException("Unsupported audio capture configuration: $bufferSize")
                }

                record = AudioRecord(
                    MediaRecorder.AudioSource.VOICE_RECOGNITION,
                    SAMPLE_RATE,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT,
                    bufferSize,
                )
                val pipeline = initPipeline(configJson)

                if (generation.get() != session) return@withContext null
                record.startRecording()
                if (generation.get() != session) return@withContext null
                active = true

                val buf = ShortArray(BUF_SAMPLES)
                val deadlineMs = SystemClock.elapsedRealtime() + maxDurationSec * 1000L

                while (
                    active &&
                    generation.get() == session &&
                    SystemClock.elapsedRealtime() < deadlineMs
                ) {
                    val nRead = record.read(buf, 0, buf.size)
                    if (nRead < 0) throw IOException("Audio capture failed: $nRead")
                    if (nRead == 0) continue

                    onLevel?.invoke(computeRmsLevel(buf, nRead))
                    pipeline.pushAudio(buf, nRead)
                }

                if (generation.get() != session) return@withContext null

                active = false
                runCatching { record.stop() }
                record.release()
                record = null
                onTranscribing?.invoke()

                pipeline.finalize().ifBlank { null }
            } finally {
                active = false
                runCatching { record?.stop() }
                record?.release()
            }
        }
    }

    fun stop() {
        active = false
    }

    fun cancel() {
        generation.incrementAndGet()
        active = false
    }

    fun cleanup() {
        cachedPipeline?.destroy()
        cachedPipeline = null
        cachedConfigJson = null
    }

    private fun initPipeline(configJson: String): BarkPipeline {
        if (configJson == cachedConfigJson) {
            cachedPipeline?.reset()
            return cachedPipeline!!
        }

        cachedPipeline?.destroy()
        cachedPipeline = BarkPipeline(configJson)
        cachedConfigJson = configJson
        return cachedPipeline!!
    }

    private fun computeRmsLevel(buffer: ShortArray, samples: Int): Float {
        var sumSquares = 0.0
        var i = 0
        while (i < samples) {
            val sample = buffer[i].toInt()
            sumSquares += (sample * sample).toDouble()
            i++
        }
        val rms = sqrt(sumSquares / samples)
        return (rms / 32768.0).toFloat().coerceIn(0f, 1f)
    }
}
