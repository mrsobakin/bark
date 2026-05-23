package com.mrsobakin.bark

import android.app.Activity
import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.TextView

class SettingsActivity : Activity() {

    private lateinit var urlInput: EditText
    private lateinit var modelInput: EditText
    private lateinit var apiKeyInput: EditText
    private lateinit var promptInput: EditText
    private lateinit var saveStatus: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_settings)

        urlInput = findViewById(R.id.endpointUrl)
        modelInput = findViewById(R.id.modelName)
        apiKeyInput = findViewById(R.id.apiKey)
        promptInput = findViewById(R.id.initialPrompt)
        saveStatus = findViewById(R.id.saveStatus)
        val saveBtn = findViewById<Button>(R.id.saveButton)

        val prefs = getSharedPreferences("bark", MODE_PRIVATE)
        prefs.getString("endpoint_url", "")?.let { url ->
            if (url.isNotEmpty()) urlInput.setText(url)
        }
        prefs.getString("model", "whisper-large-v3-turbo")?.let { model ->
            modelInput.setText(model)
        }
        prefs.getString("api_key", "")?.let { key ->
            if (key.isNotEmpty()) apiKeyInput.setText(key)
        }
        prefs.getString("prompt", "")?.let { prompt ->
            if (prompt.isNotEmpty()) promptInput.setText(prompt)
        }

        saveBtn.setOnClickListener { saveSettings() }
    }

    private fun saveSettings() {
        val url = urlInput.text.toString().trim()
        val model = modelInput.text.toString().trim()
        val apiKey = apiKeyInput.text.toString().trim()
        val prompt = promptInput.text.toString().trim()
        when {
            url.isEmpty() -> showStatus("URL cannot be empty", success = false)
            !url.startsWith("http://") && !url.startsWith("https://") ->
                showStatus("URL must start with http:// or https://", success = false)
            else -> {
                getSharedPreferences("bark", MODE_PRIVATE)
                    .edit()
                    .putString("endpoint_url", url)
                    .putString("model", model.ifEmpty { "whisper-large-v3-turbo" })
                    .putString("api_key", apiKey)
                    .putString("prompt", prompt)
                    .apply()
                showStatus(getString(R.string.saved), success = true)
            }
        }
    }

    private fun showStatus(msg: String, success: Boolean) {
        saveStatus.text = msg
        saveStatus.setTextColor(if (success) 0xFF4CAF50.toInt() else 0xFFE53935.toInt())
        saveStatus.visibility = View.VISIBLE
        saveStatus.postDelayed({ saveStatus.visibility = View.GONE }, 3000)
    }
}
