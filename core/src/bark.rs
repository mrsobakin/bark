use crate::audio::OpusEncoder;
use crate::config::BarkConfig;
use crate::engine::{TranscriptionEngine, WhisperClient};
use crate::post;
use crate::pre::Preprocessor;
use crate::Result;

pub struct Bark {
    config: BarkConfig,
    encoder: Option<OpusEncoder<Vec<u8>>>,
    preprocessor: Preprocessor,
}

impl Bark {
    pub fn new(config: BarkConfig) -> Result<Bark> {
        let preprocessor = Preprocessor::new(&config.pre)?;

        Ok(Self {
            config,
            encoder: None,
            preprocessor,
        })
    }

    pub fn push_audio(&mut self, frames: &[i16]) -> Result<()> {
        let speech = self.preprocessor.process(frames)?;

        if !speech.is_empty() {
            self.get_encoder()?.feed(&speech)?;
        }

        Ok(())
    }

    fn get_encoder(&mut self) -> Result<&mut OpusEncoder<Vec<u8>>> {
        if self.encoder.is_none() {
            self.encoder = Some(OpusEncoder::new(Vec::new())?);
        }
        Ok(self.encoder.as_mut().unwrap())
    }

    pub fn finalize(&mut self) -> Result<String> {
        let tail = self.preprocessor.finish()?;
        if !tail.is_empty() {
            self.get_encoder()?.feed(&tail)?;
        }

        let Some(encoder) = self.encoder.take() else {
            return Ok(String::new());
        };

        let audio = encoder.finish()?;
        if audio.is_empty() {
            return Ok(String::new());
        }

        let client = WhisperClient::new(&self.config.engine)?;
        let text = client.transcribe(&audio)?;

        if text.is_empty() {
            return Ok(String::new());
        }

        let final_text = post::postprocess(&text, &self.config.post);
        Ok(final_text)
    }

    pub fn reset(&mut self) {
        self.preprocessor.reset();
        self.encoder = None;
    }
}
