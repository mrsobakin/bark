package com.mrsobakin.bark

import android.content.Context
import com.konovalov.vad.silero.VadSilero
import com.konovalov.vad.silero.config.FrameSize
import com.konovalov.vad.silero.config.Mode
import com.konovalov.vad.silero.config.SampleRate
import java.io.ByteArrayOutputStream
import java.io.Closeable

class VADProcessor(
    context: Context,
    sampleRate: SampleRate = SampleRate.SAMPLE_RATE_16K,
    frameSize: FrameSize = FrameSize.FRAME_SIZE_512,
    mode: Mode = Mode.NORMAL,
    private val minSpeechFrames: Int = speechMsToFrames(100, frameSize, sampleRate),
    private val minSilenceFrames: Int = speechMsToFrames(150, frameSize, sampleRate),
    private val maxSilenceFrames: Int = speechMsToFrames(500, frameSize, sampleRate),
    private val attackFrames: Int = speechMsToFrames(150, frameSize, sampleRate),
) : Closeable {

    private val rawVad = VadSilero(
        context,
        sampleRate = sampleRate,
        frameSize = frameSize,
        mode = mode,
        speechDurationMs = 0,
        silenceDurationMs = 0,
    )

    private val zeroFrame = ByteArray(frameSize.value * 2)

    private var inSpeech = false
    private var consecutiveSpeech = 0
    private var consecutiveSilence = 0
    private var silenceFramesWritten = 0

    private val attackBuffer = ArrayDeque<ByteArray>()

    fun process(frame: ByteArray, out: ByteArrayOutputStream) {
        if (rawVad.isSpeech(frame)) {
            onSpeechFrame(frame, out)
        } else {
            onSilenceFrame(frame, out)
        }
    }

    fun reset() {
        inSpeech = false
        consecutiveSpeech = 0
        consecutiveSilence = 0
        silenceFramesWritten = 0
        attackBuffer.clear()
    }

    override fun close() {
        rawVad.close()
    }

    private fun onSpeechFrame(frame: ByteArray, out: ByteArrayOutputStream) {
        consecutiveSpeech++
        consecutiveSilence = 0

        if (inSpeech) {
            out.write(frame)
        } else {
            // Rolling pre-speech buffer so we can recover audio before the confirmation window.
            attackBuffer.addLast(frame.copyOf())
            while (attackBuffer.size > attackFrames) {
                attackBuffer.removeFirst()
            }

            if (consecutiveSpeech >= minSpeechFrames) {
                inSpeech = true
                silenceFramesWritten = 0
                attackBuffer.forEach { out.write(it) }
                attackBuffer.clear()
            }
        }
    }

    private fun onSilenceFrame(frame: ByteArray, out: ByteArrayOutputStream) {
        consecutiveSilence++
        consecutiveSpeech = 0

        if (inSpeech) {
            out.write(frame)
            if (consecutiveSilence >= minSilenceFrames) {
                inSpeech = false
                attackBuffer.clear()
            }
        } else {
            // Track real frames for potential attack buffer use on next speech transition.
            attackBuffer.addLast(frame.copyOf())
            while (attackBuffer.size > attackFrames) {
                attackBuffer.removeFirst()
            }

            if (silenceFramesWritten < maxSilenceFrames) {
                out.write(zeroFrame)
                silenceFramesWritten++
            }
            // Beyond maxSilenceFrames: frame is dropped.
        }
    }

    companion object {
        private fun speechMsToFrames(ms: Int, frameSize: FrameSize, sampleRate: SampleRate): Int =
            ms / (frameSize.value * 1000 / sampleRate.value)
    }
}
