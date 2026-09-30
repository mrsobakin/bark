package com.mrsobakin.bark

import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import com.google.android.material.button.MaterialButton
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.atomic.AtomicLong

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class BarkKeyboardServiceTest {
    private val dispatcher = StandardTestDispatcher()
    private lateinit var service: BarkKeyboardService
    private lateinit var view: View
    private lateinit var capture: AudioCapture
    private lateinit var job: Job
    private fun editor(field: Int = 42) = EditorInfo().apply {
        packageName = "com.example.editor"
        fieldId = field
        inputType = android.text.InputType.TYPE_CLASS_TEXT
    }

    @Before
    fun setUp() {
        Dispatchers.setMain(dispatcher)
        service = Robolectric.buildService(BarkKeyboardService::class.java).create().get()
        view = service.onCreateInputView()
        service.onStartInput(editor(), false)
        capture = ReflectionHelpers.getField(service, "audioCapture")
        // Keep lifecycle tests independent of microphone hardware and the native pipeline.
        job = Job()
        ReflectionHelpers.setField(service, "currentJob", job)
    }

    @After
    fun tearDown() {
        service.onDestroy()
        dispatcher.scheduler.runCurrent()
        Dispatchers.resetMain()
    }

    @Test
    fun notificationShadePreservesRecordingAndStopStillTranscribes() {
        restoreState("Recording", recording = true)
        val generation = ReflectionHelpers.getField<AtomicLong>(capture, "generation").get()

        repeat(2) {
            service.onFinishInputView(false)
            assertTrue(capture.isActive)
            assertFalse(job.isCancelled)

            service.onStartInputView(editor(), false)
            assertSame(job, ReflectionHelpers.getField(service, "currentJob"))
            assertFalse(ReflectionHelpers.getField(service, "restartPending"))
            assertEquals(generation, ReflectionHelpers.getField<AtomicLong>(capture, "generation").get())
            assertEquals(service.getString(R.string.stop_recording), micButton.contentDescription)
        }

        micButton.performClick()
        dispatcher.scheduler.runCurrent()
        assertFalse(capture.isActive)
        assertFalse(job.isCancelled)
        assertFalse(micButton.isEnabled)
        assertEquals(generation, ReflectionHelpers.getField<AtomicLong>(capture, "generation").get())
    }

    @Test
    fun notificationShadePreservesTranscriptionAndRestoresBusyUi() {
        restoreState("Transcribing")
        service.onFinishInputView(false)
        service.onStartInputView(editor(), true)

        assertFalse(job.isCancelled)
        assertSame(job, ReflectionHelpers.getField(service, "currentJob"))
        assertFalse(ReflectionHelpers.getField(service, "restartPending"))
        assertFalse(micButton.isEnabled)
    }

    @Test
    fun temporaryHideWhileAwaitingPermissionDoesNotQueueAnotherRecording() {
        service.onStartInputView(editor(), false)
        service.onFinishInputView(false)
        service.onStartInputView(editor(), false)

        assertFalse(job.isCancelled)
        assertFalse(ReflectionHelpers.getField(service, "restartPending"))
        assertSame(job, ReflectionHelpers.getField(service, "currentJob"))
    }

    @Test
    fun endingEditorSessionStillCancelsRecording() {
        restoreState("Recording", recording = true)
        service.onFinishInputView(false)
        service.onFinishInput()
        dispatcher.scheduler.runCurrent()

        assertTrue(job.isCancelled)
        assertFalse(capture.isActive)
    }

    @Test
    fun finishingInputViewStillCancelsRecording() {
        restoreState("Recording", recording = true)
        service.onFinishInputView(true)
        service.onFinishInput()
        dispatcher.scheduler.runCurrent()

        assertTrue(job.isCancelled)
        assertFalse(capture.isActive)
    }

    @Test
    fun backStillCancelsRecording() {
        restoreState("Recording", recording = true)
        service.onKeyDown(KeyEvent.KEYCODE_BACK, KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK))

        assertTrue(job.isCancelled)
        assertFalse(capture.isActive)
    }

    @Test
    fun shadeClosingWithFullEditorRestartPreservesRecording() {
        restoreState("Recording", recording = true)
        val generation = ReflectionHelpers.getField<AtomicLong>(capture, "generation").get()

        repeat(2) {
            // Exact callback sequence observed on the phone when closing the shade.
            service.onFinishInputView(true)
            service.onFinishInput()
            service.onStartInput(editor(), false)
            service.onStartInputView(editor(), false)
            dispatcher.scheduler.runCurrent()

            assertTrue(capture.isActive)
            assertFalse(job.isCancelled)
            assertSame(job, ReflectionHelpers.getField(service, "currentJob"))
            assertFalse(ReflectionHelpers.getField(service, "restartPending"))
            assertEquals(generation, ReflectionHelpers.getField<AtomicLong>(capture, "generation").get())
            assertEquals(service.getString(R.string.stop_recording), micButton.contentDescription)
        }

        micButton.performClick()
        dispatcher.scheduler.runCurrent()
        assertFalse(capture.isActive)
        assertFalse(job.isCancelled)
        assertFalse(micButton.isEnabled)
    }

    @Test
    fun fullEditorRestartPreservesTranscription() {
        restoreState("Transcribing")
        service.onFinishInputView(true)
        service.onFinishInput()
        service.onStartInput(editor(), false)
        service.onStartInputView(editor(), false)
        dispatcher.scheduler.runCurrent()

        assertFalse(job.isCancelled)
        assertFalse(micButton.isEnabled)
        assertFalse(ReflectionHelpers.getField(service, "restartPending"))
    }

    @Test
    fun reconnectingDifferentFieldCancelsPreviousRecording() {
        restoreState("Recording", recording = true)
        service.onFinishInputView(true)
        service.onFinishInput()
        service.onStartInput(editor(field = 99), false)
        dispatcher.scheduler.runCurrent()

        assertTrue(job.isCancelled)
        assertFalse(capture.isActive)
    }

    @Test
    fun reconnectingDifferentAppCancelsPreviousRecording() {
        restoreState("Recording", recording = true)
        service.onFinishInputView(true)
        service.onFinishInput()
        service.onStartInput(editor().apply { packageName = "com.example.other" }, false)
        dispatcher.scheduler.runCurrent()

        assertTrue(job.isCancelled)
        assertFalse(capture.isActive)
    }

    private val micButton: MaterialButton get() = view.findViewById(R.id.recordButton)

    private fun restoreState(name: String, recording: Boolean = false) {
        val state = Class.forName("com.mrsobakin.bark.BarkKeyboardService\$State\$$name")
            .getDeclaredField("INSTANCE").apply { isAccessible = true }.get(null)
        ReflectionHelpers.setField(service, "state", state)
        ReflectionHelpers.setField(capture, "active", recording)
        service.onStartInputView(editor(), false)
    }
}
