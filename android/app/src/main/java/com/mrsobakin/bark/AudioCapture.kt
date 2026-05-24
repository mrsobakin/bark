package com.mrsobakin.bark

import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.sqrt

/**
 * Captures audio via [AudioRecord] and pipes the raw 16-bit PCM stream
 * through [BarkPipeline] (JNI → Rust).
 *
 * The Rust side handles VAD, Opus encoding, HTTP upload, and transcription.
 * Android just records and returns the transcribed text.
 */
class AudioCapture {

    companion object {
        private const val TAG = "BarkAudio"
        private const val SAMPLE_RATE = 16000

        /** Read buffer: 512 samples × 2 bytes = 1024 bytes (one VAD frame). */
        private const val BUF_BYTES = 1024
    }

    @Volatile
    private var active = false

    val isActive: Boolean get() = active

    /** Cached pipeline reused across recordings to avoid reloading the ORT model. */
    private var cachedPipeline: BarkPipeline? = null
    private var cachedConfigJson: String? = null

    // Public API ----------------------------------------------------------

    /**
     * Record audio and return the transcribed text via Rust's pipeline.
     *
     * @param maxDurationSec  Hard time limit (default 300 s = 5 min).
     * @param configJson      JSON-serialized [BarkConfig] (VAD + engine settings).
     * @param onLevel         Called on IO thread for each processed frame with RMS level in 0..1.
     * @return The transcribed text, or `null` if no speech was detected.
     */
    suspend fun recordOnce(
        maxDurationSec: Int = 300,
        configJson: String,
        onLevel: ((Float) -> Unit)? = null,
    ): String? = withContext(Dispatchers.IO) {
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

        val pipeline = initPipeline(configJson)

        try {
            val buf = ByteArray(BUF_BYTES)
            val deadlineMs = System.currentTimeMillis() + maxDurationSec * 1000L

            // ── Capture loop: read PCM, push to JNI ────────────────
            while (active && System.currentTimeMillis() < deadlineMs) {
                val nRead = record.read(buf, 0, buf.size)
                if (nRead <= 0) continue

                onLevel?.invoke(computeRmsLevel(buf, nRead))

                // Trim to actual bytes read, pass to Rust
                val data = if (nRead < buf.size) buf.copyOf(nRead) else buf
                pipeline.pushAudio(data)
            }

            // ── Finalize: Rust flushes VAD, finishes Opus, HTTP, transcribes ──
            return@withContext pipeline.finalize().ifBlank { null }
        } finally {
            active = false
            runCatching { record.stop() }
            record.release()
        }
    }

    /** Cancel an in-progress recording. The capture loop will exit shortly after. */
    fun cancel() {
        active = false
    }

    /** Release the cached pipeline (if any). Call when the keyboard service is destroyed. */
    fun cleanup() {
        cachedPipeline?.destroy()
        cachedPipeline = null
        cachedConfigJson = null
    }

    // Private helpers ---------------------------------------------------

    /**
     * Get or create a [BarkPipeline], reusing the cached instance if the
     * config hasn't changed.  This avoids reloading the Silero ONNX model
     * between recordings.
     */
    private fun initPipeline(configJson: String): BarkPipeline {
        if (configJson == cachedConfigJson) {
            cachedPipeline?.reset()
            return cachedPipeline!!
        }

        // Config changed (or first call) — create a new pipeline.
        cachedPipeline?.destroy()
        BarkPipeline().also { pipeline ->
            pipeline.create(configJson)
            cachedPipeline = pipeline
            cachedConfigJson = configJson
        }
        return cachedPipeline!!
    }

    /** Normalised RMS (0..1) for a 16-bit PCM buffer. */
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
