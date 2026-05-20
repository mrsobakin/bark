use crate::config::EngineConfig;
use crate::engine::{TranscriptionEngine, TranscriptionError};
use reqwest::blocking::multipart;

pub struct WhisperClient {
    http: reqwest::blocking::Client,
    config: EngineConfig,
}

impl WhisperClient {
    pub fn new(config: &EngineConfig) -> Result<Self, TranscriptionError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        let config = config.clone();
        Ok(Self { http, config })
    }
}

impl TranscriptionEngine for WhisperClient {
    fn transcribe(&self, ogg_data: &[u8]) -> Result<String, TranscriptionError> {
        if ogg_data.is_empty() {
            return Ok(String::new());
        }

        let file_part = multipart::Part::bytes(ogg_data.to_vec())
            .file_name("audio.ogg".to_string())
            .mime_str("audio/ogg")
            .map_err(|e| TranscriptionError(format!("invalid MIME: {e}")))?;

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

        let resp = req.send()?;

        if !resp.status().is_success() {
            let status = resp.status();

            let mut body = resp.text().unwrap_or_default();
            body.truncate(200);
            let snippet = body.replace('\n', " ");

            return Err(TranscriptionError(format!(
                "HTTP {status}: {snippet}"
            )));
        }

        let text = resp.text()?;
        Ok(text.trim().to_string())
    }
}
