package com.mrsobakin.bark

/**
 * JNI bridge to the Rust [bark_core::Bark] pipeline.
 *
 * Handles VAD, Opus encoding, HTTP upload, and transcription in Rust.
 * Android just captures PCM audio, pushes it here, and reads the final text.
 */
class BarkPipeline : AutoCloseable {

    companion object {
        init {
            System.loadLibrary("bark_jni")
        }
    }

    private var handle: Long = 0

    /**
     * Create a new pipeline instance.
     * @param configJson JSON-serialized [bark_core::BarkConfig] with VAD and engine settings.
     * @throws IllegalStateException if creation fails.
     */
    fun create(configJson: String) {
        if (handle != 0L) destroy()
        handle = nativeCreate(configJson)
        if (handle == 0L) {
            throw IllegalStateException("BarkPipeline nativeCreate returned null handle")
        }
    }

    /** Release the native handle. Safe to call multiple times. */
    fun destroy() {
        if (handle != 0L) {
            nativeDestroy(handle)
            handle = 0L
        }
    }

    /** Reset VAD state and encoder for a new recording session. */
    fun reset() {
        if (handle == 0L) throw IllegalStateException("BarkPipeline not initialized")
        nativeReset(handle)
    }

    /**
     * Push raw 16-bit PCM audio data (little-endian bytes) to the pipeline.
     * The Rust side runs VAD and encodes speech segments as Opus.
     */
    fun pushAudio(data: ByteArray) {
        if (handle == 0L) throw IllegalStateException("BarkPipeline not initialized")
        nativePushAudio(handle, data)
    }

    /**
     * Finalize the recording: flush VAD, finish Opus stream, upload to
     * the configured Whisper endpoint, and return the transcribed text.
     *
     * @return Transcribed text, or empty string if no speech was detected.
     * @throws IllegalStateException if finalization fails.
     */
    fun finalize(): String {
        if (handle == 0L) throw IllegalStateException("BarkPipeline not initialized")
        return nativeFinalize(handle) ?: ""
    }

    override fun close() {
        destroy()
    }

    // ── Native methods ─────────────────────────────────────────────

    private external fun nativeCreate(configJson: String): Long
    private external fun nativeDestroy(handle: Long)
    private external fun nativeReset(handle: Long)
    private external fun nativePushAudio(handle: Long, data: ByteArray)
    private external fun nativeFinalize(handle: Long): String?
}
