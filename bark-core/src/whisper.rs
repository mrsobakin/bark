//! Whisper-compatible transcription engine (Groq / OpenAI API).

use crate::config::EngineConfig;
use crate::error::{BarkError, Result};
use reqwest::multipart;

/// HTTP-based transcription client for any Whisper-compatible endpoint.
pub struct WhisperClient {
    http: reqwest::Client,
    config: EngineConfig,
}

impl WhisperClient {
    pub fn new(config: EngineConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("failed to build HTTP client");

        Self { http, config }
    }

    /// Send `ogg_data` (a complete OGG/Opus file) to the transcription
    /// endpoint and return the transcribed text.
    pub async fn transcribe(&self, ogg_data: &[u8]) -> Result<String> {
        if ogg_data.is_empty() {
            return Ok(String::new());
        }

        let file_part = multipart::Part::bytes(ogg_data.to_vec())
            .file_name("audio.ogg".to_string())
            .mime_str("audio/ogg")
            .map_err(|e| BarkError::Transcription(format!("invalid MIME: {e}")))?;

        let mut form = multipart::Form::new()
            .part("file", file_part)
            .text("model", self.config.model.clone())
            .text("response_format", "text".to_string());

        if let Some(ref lang) = self.config.language {
            form = form.text("language", lang.clone());
        }
        if let Some(ref prompt) = self.config.prompt {
            form = form.text("prompt", prompt.clone());
        }

        let mut req = self.http.post(&self.config.endpoint).multipart(form);

        if !self.config.api_key.is_empty() {
            req = req.bearer_auth(&self.config.api_key);
        }

        let resp = req.send().await.map_err(BarkError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(200).collect();
            let snippet = snippet.replace('\n', " ");
            return Err(BarkError::Transcription(format!(
                "HTTP {status}: {snippet}"
            )));
        }

        let text = resp.text().await.map_err(BarkError::Http)?;
        Ok(text.trim().to_string())
    }
}