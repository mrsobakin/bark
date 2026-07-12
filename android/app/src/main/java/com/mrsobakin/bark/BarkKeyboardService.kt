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
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.io.IOException

class BarkKeyboardService : InputMethodService() {

    companion object {
        private const val TAG = "BarkKeyboard"

        val permissionResult = Channel<Boolean>(Channel.CONFLATED)
    }

    private lateinit var micButton: ImageButton
    private lateinit var levelIndicator: View
    private lateinit var transcribingIndicator: ImageView

    private val audioCapture = AudioCapture()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    private var currentJob: Job? = null
    private var inputViewActive = false
    private var restartPending = false
    private var switchBackPending = false

    private var smoothedLevel = 0f
    private val levelSmoothing = 0.3f
    private var levelAnimJob: Job? = null
    private var spinnerAnimator: ObjectAnimator? = null

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
        abortWithDiscard()
    }

    override fun onFinishInput() {
        super.onFinishInput()
        Log.d(TAG, "onFinishInput")
        inputViewActive = false
        restartPending = false
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
            }
        )

        return permissionResult.receive()
    }

    private fun onMicTapped() {
        when {
            audioCapture.isActive -> {
                Log.d(TAG, "manual stop")
                audioCapture.stop()
                updateUi(State.Transcribing)
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

    private fun onAudioLevel(raw: Float) {
        smoothedLevel = smoothedLevel * (1f - levelSmoothing) + raw * levelSmoothing
        val display = smoothedLevel

        levelIndicator.post {
            if (levelIndicator.visibility != View.VISIBLE) return@post
            val boosted = kotlin.math.sqrt(kotlin.math.sqrt(display.toDouble())).toFloat()
            val s = 0.7f + boosted * 2.8f
            levelIndicator.scaleX = s
            levelIndicator.scaleY = s
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

    private fun resetLevelIndicator() {
        levelIndicator.visibility = View.GONE
        levelIndicator.scaleX = 0.7f
        levelIndicator.scaleY = 0.7f
        levelIndicator.alpha = 0f
    }

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

    private sealed class State {
        object Recording : State()
        object Transcribing : State()
        data class Error(val message: String) : State()
        object Idle : State()
    }

    private fun updateUi(state: State) {
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
