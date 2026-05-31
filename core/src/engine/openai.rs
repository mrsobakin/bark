use crate::audio::OpusEncoder;
use crate::config::EngineConfig;
use crate::engine::{TranscriptionEngine, TranscriptionError};
use ureq::unversioned::multipart::{Form, Part};

const MIN_AUDIO_SECONDS: f32 = 0.5;

pub struct OpenAIClient {
    http: ureq::Agent,
    config: EngineConfig,
    encoder: OpusEncoder<Vec<u8>>,
}

impl OpenAIClient {
    pub fn new(config: &EngineConfig) -> Result<Self, TranscriptionError> {
        let http: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(60)))
            .http_status_as_error(false)
            .build()
            .into();
        let config = config.clone();
        let encoder = OpusEncoder::new(Vec::new())?;
        Ok(Self {
            http,
            config,
            encoder,
        })
    }

    fn transcribe_ogg(
        http: &ureq::Agent,
        config: &EngineConfig,
        ogg_data: &[u8],
    ) -> Result<String, TranscriptionError> {
        if ogg_data.is_empty() {
            return Ok(String::new());
        }

        let file_part = Part::bytes(ogg_data)
            .file_name("audio.ogg")
            .mime_str("audio/ogg")
            .map_err(|e| TranscriptionError(format!("invalid MIME: {e}")))?;

        let mut form = Form::new()
            .part("file", file_part)
            .text("model", &config.model)
            .text("response_format", "text");

        if let Some(ref lang) = config.language {
            form = form.text("language", lang);
        }
        if let Some(ref prompt) = config.prompt {
            form = form.text("prompt", prompt);
        }

        let mut req = http.post(&config.endpoint);

        if !config.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", config.api_key));
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

impl TranscriptionEngine for OpenAIClient {
    fn push_audio(&mut self, pcm: &[i16]) -> Result<(), TranscriptionError> {
        self.encoder.feed(pcm)?;
        Ok(())
    }

    fn finalize(self: Box<Self>) -> Result<String, TranscriptionError> {
        let Self {
            http,
            config,
            encoder,
        } = *self;
        let (audio, duration) = encoder.finish()?;
        if duration < MIN_AUDIO_SECONDS {
            return Ok(String::new());
        }
        Self::transcribe_ogg(&http, &config, &audio)
    }
}
