package com.mrsobakin.bark

import android.Manifest
import android.annotation.SuppressLint
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.inputmethodservice.InputMethodService
import android.util.Log
import android.view.ContextThemeWrapper
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.Toast
import androidx.interpolator.view.animation.FastOutLinearInInterpolator
import com.google.android.material.button.MaterialButton
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.io.IOException

class BarkKeyboardService : InputMethodService() {

    companion object {
        private const val TAG = "BarkKeyboard"

        val permissionResult = Channel<Boolean>(Channel.CONFLATED)
    }

    private lateinit var micButton: MaterialButton
    private lateinit var audioVisualizer: AudioReactiveBlobView

    private val audioCapture = AudioCapture()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    private var currentJob: Job? = null
    private var inputViewActive = false
    private var restartPending = false
    private var switchBackPending = false
    private var appearanceSignature = ""

    @SuppressLint("InflateParams")
    @Suppress("DEPRECATION")
    override fun onCreateInputView(): View {
        val themedContext = Appearance.wrap(ContextThemeWrapper(this, R.style.Theme_Bark))
        val view = layoutInflater.cloneInContext(themedContext).inflate(R.layout.keyboard, null)

        micButton = view.findViewById(R.id.recordButton)
        audioVisualizer = view.findViewById(R.id.audioVisualizer)
        appearanceSignature = currentAppearanceSignature()

        micButton.setOnClickListener { onMicTapped() }

        window.window?.navigationBarColor = Color.BLACK
        return view
    }

    override fun onStartInputView(info: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        Log.d(TAG, "onStartInputView restarting=$restarting")
        if (appearanceSignature != currentAppearanceSignature()) {
            setInputView(onCreateInputView())
        }
        inputViewActive = true
        switchBackPending = true
        updateUi(State.Idle)
        launchRecordingFlow()
    }

    override fun onFinishInputView(finishingInput: Boolean) {
        super.onFinishInputView(finishingInput)
        Log.d(TAG, "onFinishInputView finishingInput=$finishingInput")
        inputViewActive = false
        restartPending = false
        audioVisualizer.stop()
        abortWithDiscard()
    }

    override fun onFinishInput() {
        super.onFinishInput()
        Log.d(TAG, "onFinishInput")
        inputViewActive = false
        restartPending = false
        if (::audioVisualizer.isInitialized) audioVisualizer.stop()
        abortWithDiscard()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            Log.d(TAG, "back pressed")
            abortWithDiscard()
            scheduleSwitchBack()
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    override fun onDestroy() {
        super.onDestroy()
        val job = currentJob
        abortWithDiscard()
        if (job == null || job.isCompleted) {
            audioCapture.cleanup()
        } else {
            job.invokeOnCompletion { audioCapture.cleanup() }
        }
        scope.cancel()
    }

    private fun micGranted() =
        checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

    private suspend fun awaitMicPermission(): Boolean {
        permissionResult.tryReceive()

        startActivity(
            Intent(this, PermissionBridgeActivity::class.java).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            },
        )

        return permissionResult.receive()
    }

    private fun onMicTapped() {
        when {
            audioCapture.isActive -> {
                Log.d(TAG, "manual stop")
                audioCapture.stop()
                markTranscribing()
            }
            currentJob?.isCompleted == false -> {
                Log.d(TAG, "tap ignored — flow in progress")
            }
            else -> {
                Log.d(TAG, "manual start")
                launchRecordingFlow()
            }
        }
    }

    private suspend fun recordAndTranscribeFlow() {
        if (!micGranted()) {
            updateUi(State.Idle)
            val granted = awaitMicPermission()
            if (!granted) {
                updateUi(State.Error(getString(R.string.mic_permission)))
                delay(1500)
                updateUi(State.Idle)
                return
            }
        }

        val configJson: String
        try {
            configJson = buildConfigJson()
        } catch (e: IOException) {
            updateUi(State.Error(e.message ?: getString(R.string.error_no_endpoint)))
            delay(3000)
            updateUi(State.Idle)
            scheduleSwitchBack()
            return
        }

        updateUi(State.Recording)
        val text: String?
        try {
            text = audioCapture.recordOnce(
                configJson = configJson,
                onLevel = audioVisualizer::setLevel,
                onTranscribing = { markTranscribing() },
            )
        } catch (_: CancellationException) {
            return
        } catch (e: Exception) {
            Log.e(TAG, "recording/transcription failed", e)
            val className = e::class.simpleName ?: "Exception"
            val msg = e.message?.let { "$className: $it" } ?: className
            updateUi(State.Error("${getString(R.string.error_transcription)}: $msg"))
            delay(3000)
            updateUi(State.Idle)
            scheduleSwitchBack()
            return
        }

        currentCoroutineContext().ensureActive()

        if (!text.isNullOrBlank()) {
            commitText(text)
        }

        updateUi(State.Idle)
        scheduleSwitchBack()
    }

    private fun buildConfigJson(): String {
        val prefs = getSharedPreferences("bark", MODE_PRIVATE)
        val url = prefs.getString("endpoint_url", "").orEmpty()
        val model = prefs.getString("model", "whisper-large-v3-turbo").orEmpty()
        val apiKey = prefs.getString("api_key", "").orEmpty()
        val prompt = prefs.getString("prompt", "").orEmpty()

        if (url.isEmpty()) throw IOException(getString(R.string.error_no_endpoint))

        val engine = JSONObject().apply {
            put("endpoint", url)
            put("api_key", apiKey)
            put("model", model.ifEmpty { "whisper-large-v3-turbo" })
            if (prompt.isNotEmpty()) put("prompt", prompt)
        }

        val vad = JSONObject().apply {
            put("threshold", 0.3)
            put("min_speech_ms", 100)
            put("min_silence_ms", 150)
            put("max_silence_ms", 500)
            put("attack_ms", 200)
        }

        val pre = JSONObject().apply {
            put("vad", vad)
            if (prefs.getBoolean(PREF_AGC, false)) {
                put(
                    "agc",
                    JSONObject().apply {
                        put("target_db", -20.0)
                        put("max_gain_db", 12.0)
                        put("attack_ms", 30.0)
                        put("release_ms", 250.0)
                        put("rms_window_ms", 80.0)
                        put("long_window_ms", 1500.0)
                        put("high_pass_hz", 80.0)
                    },
                )
            }
        }

        return JSONObject().apply {
            put("pre", pre)
            put("engine", engine)
        }.toString()
    }

    private fun commitText(text: String) {
        val inputConnection = currentInputConnection ?: return
        inputConnection.commitText(text, 1)
        Log.d(TAG, "committed: \"$text\"")
    }

    private fun currentAppearanceSignature(): String =
        Appearance.dynamicColors(this).toString()

    private fun markTranscribing() {
        scope.launch { updateUi(State.Transcribing) }
    }

    private fun launchRecordingFlow() {
        if (currentJob?.isCompleted == false) {
            restartPending = true
            return
        }

        restartPending = false
        val job = scope.launch { recordAndTranscribeFlow() }
        currentJob = job
        job.invokeOnCompletion {
            scope.launch {
                if (restartPending && inputViewActive) launchRecordingFlow()
            }
        }
    }

    private fun abortWithDiscard() {
        currentJob?.cancel()
        audioCapture.cancel()
    }

    private fun scheduleSwitchBack() {
        if (!switchBackPending) return
        switchBackPending = false
        switchToPreviousInputMethod()
    }

    private fun updateUi(state: State) {
        micButton.animate().cancel()

        when (state) {
            State.Recording -> {
                micButton.visibility = View.VISIBLE
                micButton.alpha = 1f
                micButton.scaleX = 1f
                micButton.scaleY = 1f
                micButton.isEnabled = true
                micButton.setIconResource(R.drawable.ic_mic_active)
                micButton.contentDescription = getString(R.string.stop_recording)
                audioVisualizer.visibility = View.VISIBLE
                audioVisualizer.start()
            }
            State.Transcribing -> {
                audioVisualizer.visibility = View.VISIBLE
                audioVisualizer.startTranscribing()
                micButton.isEnabled = false
                micButton.animate()
                    .alpha(0f)
                    .scaleX(0.72f)
                    .scaleY(0.72f)
                    .setDuration(AudioReactiveBlobView.TRANSITION_PEAK_MS)
                    .setInterpolator(FastOutLinearInInterpolator())
                    .withEndAction { micButton.visibility = View.INVISIBLE }
                    .start()
            }
            is State.Error -> {
                resetIdleUi()
                Toast.makeText(this, state.message, Toast.LENGTH_SHORT).show()
            }
            State.Idle -> resetIdleUi()
        }
    }

    private fun resetIdleUi() {
        audioVisualizer.stop()
        audioVisualizer.visibility = View.INVISIBLE
        micButton.visibility = View.VISIBLE
        micButton.alpha = 1f
        micButton.scaleX = 1f
        micButton.scaleY = 1f
        micButton.isEnabled = true
        micButton.setIconResource(R.drawable.ic_mic)
        micButton.contentDescription = getString(R.string.start_recording)
    }

    private sealed class State {
        data object Recording : State()
        data object Transcribing : State()
        data class Error(val message: String) : State()
        data object Idle : State()
    }
}
