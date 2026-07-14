package com.mrsobakin.bark

class BarkPipeline(
    configJson: String,
) : AutoCloseable {

    companion object {
        init {
            System.loadLibrary("bark_jni")
        }

        fun regexError(pattern: String): String? = nativeValidateRegex(pattern)

        @JvmStatic
        private external fun nativeValidateRegex(pattern: String): String?
    }

    private var nativeHandle: Long = 0

    init {
        nativeCreate(configJson)
        check(nativeHandle != 0L) { "BarkPipeline nativeCreate returned null handle" }
    }

    @Synchronized
    fun destroy() {
        nativeDestroy()
    }

    @Synchronized
    fun reset() {
        checkOpen()
        nativeReset()
    }

    @Synchronized
    fun pushAudio(data: ShortArray, samples: Int) {
        checkOpen()
        require(samples in 0..data.size) { "Invalid sample count: $samples" }
        nativePushAudio(data, samples)
    }

    @Synchronized
    fun finalize(): String {
        checkOpen()
        return nativeFinalize() ?: ""
    }

    @Synchronized
    override fun close() {
        destroy()
    }

    private fun checkOpen() {
        check(nativeHandle != 0L) { "BarkPipeline not initialized" }
    }

    private external fun nativeCreate(configJson: String)
    private external fun nativeDestroy()
    private external fun nativeReset()
    private external fun nativePushAudio(data: ShortArray, samples: Int)
    private external fun nativeFinalize(): String?
}
