package com.mrsobakin.bark

import android.Manifest
import android.animation.ObjectAnimator
import android.animation.ValueAnimator
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.inputmethodservice.InputMethodService
import android.net.Uri
import android.provider.Settings
import android.util.Log
import android.view.KeyEvent
import android.view.View
import android.view.animation.LinearInterpolator
import android.view.inputmethod.EditorInfo
import android.widget.ImageButton
import android.widget.ImageView
import android.widget.Toast
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONObject
import java.io.IOException

/**
 * Auto-records when the keyboard opens; tap to stop, transcribe, commit, and switch back.
 *
 * Audio capture, VAD, Opus encoding, HTTP upload, and transcription are all
 * handled in Rust via JNI.  Android is just the UI shell.
 */
class BarkKeyboardService : InputMethodService() {

    companion object {
        private const val TAG = "BarkKeyboard"
        private const val PERMISSION_TIMEOUT_MS = 5_000L

        /** One-shot channel for [PermissionBridgeActivity]. */
        val permissionResult = Channel<Boolean>(Channel.CONFLATED)
    }

    // UI

    private lateinit var micButton: ImageButton
    private lateinit var levelIndicator: View
    private lateinit var transcribingIndicator: ImageView

    // State

    private val audioCapture = AudioCapture()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    /** Non-null while a recording/transcribing flow is running. */
    private var currentJob: Job? = null

    /** Switch back after the current flow completes? */
    private var switchBackPending = false

    // Level-indicator state

    private var smoothedLevel = 0f
    private val levelSmoothing = 0.3f           // lower = smoother
    private var levelAnimJob: Job? = null

    // Spinner state

    private var spinnerAnimator: ObjectAnimator? = null

    // IME lifecycle

    override fun onCreateInputView(): View {
        val v = layoutInflater.inflate(R.layout.keyboard, null)
        micButton = v.findViewById(R.id.recordButton)
        levelIndicator = v.findViewById(R.id.levelIndicator)
        transcribingIndicator = v.findViewById(R.id.transcribingIndicator)
        micButton.setOnClickListener { onMicTapped() }

        window.window?.setNavigationBarColor(Color.BLACK)
        return v
    }

    override fun onStartInputView(info: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        Log.d(TAG, "onStartInputView restarting=$restarting")
        switchBackPending = true
        updateUi(State.Idle)

        if (currentJob?.isActive == true) return
        currentJob = scope.launch { recordAndTranscribeFlow() }
    }

    override fun onFinishInput() {
        super.onFinishInput()
        Log.d(TAG, "onFinishInput")
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
        abortWithDiscard()
        audioCapture.cleanup()
        scope.cancel()
    }

    // Permission

    private fun micGranted() =
        checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

    private suspend fun awaitMicPermission(): Boolean {
        permissionResult.tryReceive()

        startActivity(
            Intent(this, PermissionBridgeActivity::class.java).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
        )

        return withTimeoutOrNull(PERMISSION_TIMEOUT_MS) { permissionResult.receive() } ?: false
    }

    // Mic button — tap toggles recording

    private fun onMicTapped() {
        when {
            audioCapture.isActive -> {
                // Tapping while recording → stop and transcribe
                Log.d(TAG, "manual stop")
                audioCapture.cancel()
                updateUi(State.Transcribing)
            }
            currentJob?.isActive == true -> {
                // Already transcribing — ignore
                Log.d(TAG, "tap ignored — flow in progress")
            }
            else -> {
                Log.d(TAG, "manual start")
                currentJob = scope.launch { recordAndTranscribeFlow() }
            }
        }
    }

    // Recording → Transcribe → Commit → Switch back

    private suspend fun recordAndTranscribeFlow() {
        // ---- 1. Permission ----
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

        // ---- 2. Build config from settings ----
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

        // ---- 3. Record + transcribe (everything in Rust via JNI) ----
        updateUi(State.Recording)
        val text: String?
        try {
            text = audioCapture.recordOnce(
                configJson = configJson,
                onLevel = { level -> onAudioLevel(level) },
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

        if (currentJob?.isActive != true) return

        // ---- 4. Commit transcribed text ----
        if (!text.isNullOrBlank()) {
            commitText(text)
        }

        updateUi(State.Idle)
        scheduleSwitchBack()
    }

    /** Build the JSON config for [BarkConfig] from shared preferences. */
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
            put("threshold", 0.5)
            put("min_speech_ms", 100)
            put("min_silence_ms", 150)
            put("max_silence_ms", 500)
            put("attack_ms", 150)
        }

        val pre = JSONObject().apply {
            put("vad", vad)
        }

        return JSONObject().apply {
            put("pre", pre)
            put("engine", engine)
        }.toString()
    }

    private fun commitText(text: String) {
        val ic = currentInputConnection ?: return
        ic.commitText(text, 1)
        Log.d(TAG, "committed: \"$text\"")
    }

    // ---- Level indicator (recording pulse) ----

    private fun onAudioLevel(raw: Float) {
        // Exponential moving average on the IO thread; UI posted below.
        smoothedLevel = smoothedLevel * (1f - levelSmoothing) + raw * levelSmoothing
        val display = smoothedLevel

        levelIndicator.post {
            if (levelIndicator.visibility != View.VISIBLE) return@post
            // 4th root: much more sensitive to quiet sounds than sqrt.
            val boosted = kotlin.math.sqrt(kotlin.math.sqrt(display.toDouble())).toFloat()
            // Scale from 0.7x (silent) up to ~3.5x (loud).
            val s = 0.7f + boosted * 2.8f
            levelIndicator.scaleX = s
            levelIndicator.scaleY = s
            // Subtle glow — max alpha 0.35 keeps it from washing out.
            levelIndicator.alpha = boosted * 0.35f
        }
    }

    private fun startLevelAnimation() {
        smoothedLevel = 0f
        levelIndicator.visibility = View.VISIBLE
        levelIndicator.scaleX = 0.7f
        levelIndicator.scaleY = 0.7f
        levelIndicator.alpha = 0f

        levelAnimJob?.cancel()
        levelAnimJob = scope.launch {
            while (isActive) {
                delay(150)
                if (smoothedLevel < 0.003f) {
                    levelIndicator.post {
                        levelIndicator.alpha = 0f
                        levelIndicator.scaleX = 0.7f
                        levelIndicator.scaleY = 0.7f
                    }
                }
            }
        }
    }

    private fun stopLevelAnimation() {
        levelAnimJob?.cancel()
        levelAnimJob = null
    }

    /** Synchronous teardown — must be called on main thread. */
    private fun resetLevelIndicator() {
        levelIndicator.visibility = View.GONE
        levelIndicator.scaleX = 0.7f
        levelIndicator.scaleY = 0.7f
        levelIndicator.alpha = 0f
    }

    // ---- Spinner (transcribing state) ----

    private fun startSpinner() {
        micButton.visibility = View.INVISIBLE
        transcribingIndicator.visibility = View.VISIBLE
        spinnerAnimator?.cancel()

        val anim = ObjectAnimator.ofFloat(transcribingIndicator, View.ROTATION, 0f, 360f)
        anim.duration = 1200
        anim.interpolator = LinearInterpolator()
        anim.repeatCount = ValueAnimator.INFINITE
        anim.start()
        spinnerAnimator = anim
    }

    private fun stopSpinner() {
        spinnerAnimator?.cancel()
        spinnerAnimator = null
        transcribingIndicator.visibility = View.GONE
        transcribingIndicator.rotation = 0f
        micButton.visibility = View.VISIBLE
    }

    // Cleanup / switch back

    private fun abortWithDiscard() {
        currentJob?.cancel()
        audioCapture.cancel()
        currentJob = null
    }

    private fun scheduleSwitchBack() {
        if (!switchBackPending) return
        switchBackPending = false
        switchToPreviousInputMethod()
    }

    // UI — fully drives all visual state

    private sealed class State {
        object Recording : State()
        object Transcribing : State()
        data class Error(val message: String) : State()
        object Idle : State()
    }

    private fun updateUi(state: State) {
        // Synchronous teardown — no post() or async calls
        stopSpinner()
        stopLevelAnimation()
        resetLevelIndicator()

        when (state) {
            is State.Recording -> {
                micButton.setImageResource(R.drawable.ic_mic_active)
                micButton.visibility = View.VISIBLE
                startLevelAnimation()
            }

            is State.Transcribing -> {
                micButton.setImageResource(R.drawable.ic_mic)
                startSpinner()
            }

            is State.Error -> {
                micButton.setImageResource(R.drawable.ic_mic)
                micButton.visibility = View.VISIBLE
                Toast.makeText(this, state.message, Toast.LENGTH_SHORT).show()
            }

            is State.Idle -> {
                micButton.setImageResource(R.drawable.ic_mic)
                micButton.visibility = View.VISIBLE
            }
        }
    }

    private fun openAppPermissions() {
        startActivity(
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
                data = Uri.fromParts("package", packageName, null)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
        )
    }
}
