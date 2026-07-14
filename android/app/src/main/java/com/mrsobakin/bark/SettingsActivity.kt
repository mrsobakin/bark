package com.mrsobakin.bark

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.edit
import androidx.core.net.toUri
import com.google.android.material.color.DynamicColors
import com.google.android.material.materialswitch.MaterialSwitch
import com.google.android.material.snackbar.Snackbar
import com.google.android.material.textfield.TextInputEditText
import com.google.android.material.textfield.TextInputLayout

class SettingsActivity : AppCompatActivity() {

    private lateinit var urlLayout: TextInputLayout
    private lateinit var urlInput: TextInputEditText
    private lateinit var modelInput: TextInputEditText
    private lateinit var apiKeyInput: TextInputEditText
    private lateinit var promptInput: TextInputEditText
    private lateinit var automaticGain: MaterialSwitch
    private lateinit var dynamicColors: MaterialSwitch

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Appearance.dynamicColors(this)) {
            DynamicColors.applyToActivityIfAvailable(this)
        }
        setContentView(R.layout.activity_settings)

        urlLayout = findViewById(R.id.endpointLayout)
        urlInput = findViewById(R.id.endpointUrl)
        modelInput = findViewById(R.id.modelName)
        apiKeyInput = findViewById(R.id.apiKey)
        promptInput = findViewById(R.id.initialPrompt)
        automaticGain = findViewById(R.id.automaticGain)
        dynamicColors = findViewById(R.id.dynamicColors)

        val prefs = getSharedPreferences("bark", MODE_PRIVATE)
        urlInput.setText(prefs.getString("endpoint_url", ""))
        modelInput.setText(prefs.getString("model", "whisper-large-v3-turbo"))
        apiKeyInput.setText(prefs.getString("api_key", ""))
        promptInput.setText(prefs.getString("prompt", ""))
        automaticGain.isChecked = prefs.getBoolean(PREF_AGC, false)
        dynamicColors.isChecked = Appearance.dynamicColors(this)

        urlInput.setOnFocusChangeListener { _, hasFocus ->
            if (hasFocus) urlLayout.error = null
        }
        findViewById<com.google.android.material.button.MaterialButton>(R.id.saveButton)
            .setOnClickListener { saveSettings() }
    }

    private fun saveSettings() {
        val url = urlInput.text?.toString()?.trim().orEmpty()
        val model = modelInput.text?.toString()?.trim().orEmpty()
        val apiKey = apiKeyInput.text?.toString()?.trim().orEmpty()
        val prompt = promptInput.text?.toString()?.trim().orEmpty()

        if (!isHttpUrl(url)) {
            urlLayout.error = getString(R.string.invalid_url)
            urlInput.requestFocus()
            return
        }

        getSharedPreferences("bark", MODE_PRIVATE).edit {
            putString("endpoint_url", url)
            putString("model", model.ifEmpty { "whisper-large-v3-turbo" })
            putString("api_key", apiKey)
            putString("prompt", prompt)
            putBoolean(PREF_AGC, automaticGain.isChecked)
            putBoolean(Appearance.PREF_DYNAMIC_COLORS, dynamicColors.isChecked)
        }

        urlLayout.error = null
        Snackbar.make(urlInput, R.string.saved, Snackbar.LENGTH_SHORT).show()
    }

    private fun isHttpUrl(value: String): Boolean {
        val uri = value.toUri()
        return uri.scheme in setOf("http", "https") && !uri.host.isNullOrBlank()
    }
}
