package com.mrsobakin.bark

import android.content.SharedPreferences
import android.os.Bundle
import android.view.MenuItem
import android.view.View
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.edit
import androidx.core.net.toUri
import androidx.core.widget.doAfterTextChanged
import androidx.recyclerview.widget.ItemTouchHelper
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.button.MaterialButton
import com.google.android.material.color.DynamicColors
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.materialswitch.MaterialSwitch
import com.google.android.material.snackbar.Snackbar
import com.google.android.material.textfield.TextInputEditText
import com.google.android.material.textfield.TextInputLayout

class SettingsActivity : AppCompatActivity() {

    companion object {
        private const val DEFAULT_MODEL = "whisper-large-v3-turbo"
        private const val STATE_POST_PROCESSORS = "postprocessors"
    }

    private lateinit var saveAction: MenuItem
    private var savedSettings: SettingsState? = null

    private lateinit var urlLayout: TextInputLayout
    private lateinit var urlInput: TextInputEditText
    private lateinit var modelInput: TextInputEditText
    private lateinit var apiKeyInput: TextInputEditText
    private lateinit var promptInput: TextInputEditText
    private lateinit var automaticGain: MaterialSwitch
    private lateinit var voiceActivityDetection: MaterialSwitch
    private lateinit var dynamicColors: MaterialSwitch
    private lateinit var postprocessorList: RecyclerView
    private lateinit var emptyPostprocessors: View
    private lateinit var postprocessorAdapter: PostProcessorAdapter
    private lateinit var postprocessorTouchHelper: ItemTouchHelper

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Appearance.dynamicColors(this)) {
            DynamicColors.applyToActivityIfAvailable(this)
        }
        setContentView(R.layout.activity_settings)

        val toolbar: MaterialToolbar = findViewById(R.id.toolbar)
        saveAction = toolbar.menu.findItem(R.id.saveSettings)
        toolbar.setOnMenuItemClickListener { item ->
            if (item.itemId == R.id.saveSettings) {
                saveSettings()
                true
            } else {
                false
            }
        }

        urlLayout = findViewById(R.id.endpointLayout)
        urlInput = findViewById(R.id.endpointUrl)
        modelInput = findViewById(R.id.modelName)
        apiKeyInput = findViewById(R.id.apiKey)
        promptInput = findViewById(R.id.initialPrompt)
        automaticGain = findViewById(R.id.automaticGain)
        voiceActivityDetection = findViewById(R.id.voiceActivityDetection)
        dynamicColors = findViewById(R.id.dynamicColors)
        postprocessorList = findViewById(R.id.postprocessorList)
        emptyPostprocessors = findViewById(R.id.emptyPostprocessors)

        postprocessorAdapter = PostProcessorAdapter(
            onEdit = ::editRegexStep,
            onRemove = ::removePostprocessor,
            onDragStarted = { postprocessorTouchHelper.startDrag(it) },
            onChanged = { empty ->
                emptyPostprocessors.visibility = if (empty) View.VISIBLE else View.GONE
                updateSaveAction()
            },
        )
        postprocessorList.layoutManager = LinearLayoutManager(this)
        postprocessorList.adapter = postprocessorAdapter
        postprocessorTouchHelper = ItemTouchHelper(
            object : ItemTouchHelper.SimpleCallback(ItemTouchHelper.UP or ItemTouchHelper.DOWN, 0) {
                override fun onMove(
                    recyclerView: RecyclerView,
                    source: RecyclerView.ViewHolder,
                    target: RecyclerView.ViewHolder,
                ): Boolean = postprocessorAdapter.move(
                    source.bindingAdapterPosition,
                    target.bindingAdapterPosition,
                )

                override fun onSwiped(viewHolder: RecyclerView.ViewHolder, direction: Int) = Unit

                override fun clearView(
                    recyclerView: RecyclerView,
                    viewHolder: RecyclerView.ViewHolder,
                ) {
                    super.clearView(recyclerView, viewHolder)
                    postprocessorAdapter.finishMove()
                }

                override fun isLongPressDragEnabled(): Boolean = false
            },
        )
        postprocessorTouchHelper.attachToRecyclerView(postprocessorList)

        val prefs = getSharedPreferences("bark", MODE_PRIVATE)
        urlInput.setText(prefs.getString("endpoint_url", ""))
        modelInput.setText(prefs.getString("model", DEFAULT_MODEL))
        apiKeyInput.setText(prefs.getString("api_key", ""))
        promptInput.setText(prefs.getString("prompt", ""))
        automaticGain.isChecked = prefs.getBoolean(PREF_AGC, false)
        voiceActivityDetection.isChecked = prefs.getBoolean(PREF_VAD, true)
        dynamicColors.isChecked = Appearance.dynamicColors(this)
        postprocessorAdapter.replaceAll(
            PipelineSettings.decodePostProcessors(
                savedInstanceState?.getString(STATE_POST_PROCESSORS)
                    ?: prefs.getString(PREF_POST_PROCESSORS, null),
            ),
        )

        savedSettings = settingsFromPreferences(prefs)
        watchForChanges()

        urlInput.setOnFocusChangeListener { _, hasFocus ->
            if (hasFocus) urlLayout.error = null
        }
        findViewById<MaterialButton>(R.id.addPostprocessor)
            .setOnClickListener { showAddPostprocessorDialog() }
        updateSaveAction()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        outState.putString(
            STATE_POST_PROCESSORS,
            PipelineSettings.encodePostProcessors(postprocessorAdapter.snapshot()),
        )
        super.onSaveInstanceState(outState)
    }

    private fun saveSettings() {
        val settings = currentSettings()
        if (!isHttpUrl(settings.url)) {
            urlLayout.error = getString(R.string.invalid_url)
            urlInput.requestFocus()
            return
        }

        val model = settings.model.ifEmpty { DEFAULT_MODEL }
        getSharedPreferences("bark", MODE_PRIVATE).edit {
            putString("endpoint_url", settings.url)
            putString("model", model)
            putString("api_key", settings.apiKey)
            putString("prompt", settings.prompt)
            putBoolean(PREF_AGC, settings.automaticGain)
            putBoolean(PREF_VAD, settings.voiceActivityDetection)
            putString(PREF_POST_PROCESSORS, settings.postProcessors)
            putBoolean(Appearance.PREF_DYNAMIC_COLORS, settings.dynamicColors)
        }

        if (settings.model.isEmpty()) modelInput.setText(model)
        savedSettings = currentSettings()
        updateSaveAction()
        urlLayout.error = null
        Snackbar.make(urlInput, R.string.saved, Snackbar.LENGTH_SHORT).show()
    }

    private fun watchForChanges() {
        listOf(urlInput, modelInput, apiKeyInput, promptInput).forEach { input ->
            input.doAfterTextChanged { updateSaveAction() }
        }
        listOf(automaticGain, voiceActivityDetection, dynamicColors).forEach { toggle ->
            toggle.setOnCheckedChangeListener { _, _ -> updateSaveAction() }
        }
    }

    private fun updateSaveAction() {
        saveAction.isEnabled = savedSettings?.let { currentSettings() != it } ?: false
    }

    private fun currentSettings() = SettingsState(
        url = urlInput.text?.toString()?.trim().orEmpty(),
        model = modelInput.text?.toString()?.trim().orEmpty(),
        apiKey = apiKeyInput.text?.toString()?.trim().orEmpty(),
        prompt = promptInput.text?.toString()?.trim().orEmpty(),
        automaticGain = automaticGain.isChecked,
        voiceActivityDetection = voiceActivityDetection.isChecked,
        postProcessors = PipelineSettings.encodePostProcessors(postprocessorAdapter.snapshot()),
        dynamicColors = dynamicColors.isChecked,
    )

    private fun settingsFromPreferences(prefs: SharedPreferences) = SettingsState(
        url = prefs.getString("endpoint_url", "").orEmpty().trim(),
        model = prefs.getString("model", DEFAULT_MODEL).orEmpty().trim(),
        apiKey = prefs.getString("api_key", "").orEmpty().trim(),
        prompt = prefs.getString("prompt", "").orEmpty().trim(),
        automaticGain = prefs.getBoolean(PREF_AGC, false),
        voiceActivityDetection = prefs.getBoolean(PREF_VAD, true),
        postProcessors = PipelineSettings.encodePostProcessors(
            PipelineSettings.decodePostProcessors(prefs.getString(PREF_POST_PROCESSORS, null)),
        ),
        dynamicColors = prefs.getBoolean(Appearance.PREF_DYNAMIC_COLORS, true),
    )

    private fun showAddPostprocessorDialog() {
        MaterialAlertDialogBuilder(this)
            .setTitle(R.string.add_processing_step)
            .setItems(arrayOf(getString(R.string.normalize), getString(R.string.regex_replace))) { _, which ->
                when (which) {
                    0 -> postprocessorAdapter.add(PostProcessorStep.Normalize)
                    1 -> showRegexDialog(null) { postprocessorAdapter.add(it) }
                }
            }
            .setNegativeButton(R.string.cancel, null)
            .show()
    }

    private fun editRegexStep(position: Int, step: PostProcessorStep.Regex) {
        showRegexDialog(step) { postprocessorAdapter.update(position, it) }
    }

    private fun showRegexDialog(
        current: PostProcessorStep.Regex?,
        onSave: (PostProcessorStep.Regex) -> Unit,
    ) {
        val view = layoutInflater.inflate(R.layout.dialog_regex_step, null)
        val patternLayout: TextInputLayout = view.findViewById(R.id.regexPatternLayout)
        val pattern: TextInputEditText = view.findViewById(R.id.regexPattern)
        val replacement: TextInputEditText = view.findViewById(R.id.regexReplacement)
        pattern.setText(current?.pattern.orEmpty())
        replacement.setText(current?.replacement.orEmpty())
        pattern.setOnFocusChangeListener { _, hasFocus ->
            if (hasFocus) patternLayout.error = null
        }

        val dialog = MaterialAlertDialogBuilder(this)
            .setTitle(if (current == null) R.string.regex_replace else R.string.edit_regex_step)
            .setView(view)
            .setNegativeButton(R.string.cancel, null)
            .setPositiveButton(if (current == null) R.string.add else R.string.done, null)
            .create()
        dialog.setOnShowListener {
            dialog.getButton(androidx.appcompat.app.AlertDialog.BUTTON_POSITIVE).setOnClickListener {
                val patternText = pattern.text?.toString().orEmpty()
                val error = BarkPipeline.regexError(patternText)
                patternLayout.error = error
                if (error == null) {
                    onSave(
                        PostProcessorStep.Regex(
                            pattern = patternText,
                            replacement = replacement.text?.toString().orEmpty(),
                        ),
                    )
                    dialog.dismiss()
                }
            }
        }
        dialog.show()
    }

    private fun removePostprocessor(position: Int) {
        val removed = postprocessorAdapter.remove(position) ?: return
        Snackbar.make(postprocessorList, R.string.step_removed, Snackbar.LENGTH_LONG)
            .setAction(R.string.undo) { postprocessorAdapter.insert(position, removed) }
            .show()
    }

    private fun isHttpUrl(value: String): Boolean {
        val uri = value.toUri()
        return uri.scheme in setOf("http", "https") && !uri.host.isNullOrBlank()
    }

    private data class SettingsState(
        val url: String,
        val model: String,
        val apiKey: String,
        val prompt: String,
        val automaticGain: Boolean,
        val voiceActivityDetection: Boolean,
        val postProcessors: String,
        val dynamicColors: Boolean,
    )
}
