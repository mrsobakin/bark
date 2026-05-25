package com.mrsobakin.bark

import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.sqrt

class AudioCapture {

    companion object {
        private const val TAG = "BarkAudio"
        private const val SAMPLE_RATE = 16000

        private const val BUF_SAMPLES = 512
    }

    @Volatile
    private var active = false

    val isActive: Boolean get() = active

    private var cachedPipeline: BarkPipeline? = null
    private var cachedConfigJson: String? = null

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
            val buf = ShortArray(BUF_SAMPLES)
            val deadlineMs = System.currentTimeMillis() + maxDurationSec * 1000L

            while (active && System.currentTimeMillis() < deadlineMs) {
                val nRead = record.read(buf, 0, buf.size)
                if (nRead <= 0) continue

                onLevel?.invoke(computeRmsLevel(buf, nRead))
                pipeline.pushAudio(buf, nRead)
            }

            return@withContext pipeline.finalize().ifBlank { null }
        } finally {
            active = false
            runCatching { record.stop() }
            record.release()
        }
    }

    fun cancel() {
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
