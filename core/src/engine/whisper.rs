use crate::config::EngineConfig;
use crate::engine::{TranscriptionEngine, TranscriptionError};
use ureq::unversioned::multipart::{Form, Part};

pub struct WhisperClient {
    http: ureq::Agent,
    config: EngineConfig,
}

impl WhisperClient {
    pub fn new(config: &EngineConfig) -> Result<Self, TranscriptionError> {
        let http: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(60)))
            .http_status_as_error(false)
            .build()
            .into();
        let config = config.clone();
        Ok(Self { http, config })
    }
}

impl TranscriptionEngine for WhisperClient {
    fn transcribe(&self, ogg_data: &[u8]) -> Result<String, TranscriptionError> {
        if ogg_data.is_empty() {
            return Ok(String::new());
        }

        let file_part = Part::bytes(ogg_data)
            .file_name("audio.ogg")
            .mime_str("audio/ogg")
            .map_err(|e| TranscriptionError(format!("invalid MIME: {e}")))?;

        let mut form = Form::new()
            .part("file", file_part)
            .text("model", &self.config.model)
            .text("response_format", "text");

        if let Some(ref lang) = self.config.language {
            form = form.text("language", lang);
        }
        if let Some(ref prompt) = self.config.prompt {
            form = form.text("prompt", prompt);
        }

        let mut req = self.http.post(&self.config.endpoint);

        if !self.config.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.config.api_key));
        }

        let mut resp = req.send(form)?;

        if !resp.status().is_success() {
            let status = resp.status();

            let mut body = resp.body_mut().read_to_string()?;
            body.truncate(200);
            let snippet = body.replace('\n', " ");

            return Err(TranscriptionError(format!("HTTP {status}: {snippet}")));
        }

        let text = resp.body_mut().read_to_string()?;
        Ok(text.trim().to_string())
    }
}
