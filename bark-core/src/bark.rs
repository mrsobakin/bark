use crate::audio::OpusEncoder;
use crate::config::BarkConfig;
use crate::engine::{TranscriptionEngine, WhisperClient};
use crate::Result;
use crate::post;
use crate::pre::Agc;
use crate::pre::vad::VadProcessor;

pub struct Bark {
    config: BarkConfig,
    encoder: Option<OpusEncoder<Vec<u8>>>,
    agc: Option<Agc>,
    vad: Option<VadProcessor>,
}

impl Bark {
    pub fn new(config: BarkConfig) -> Result<Bark> {
        let agc = config.pre.agc.as_ref().map(Agc::new);
        let vad = config.pre.vad.as_ref().map(VadProcessor::new).transpose()?;

        Ok(Self {
            config,
            encoder: None,
            agc,
            vad,
        })
    }

    pub fn push_audio(&mut self, frames: &[i16]) -> Result<()> {
        let mut data = frames.to_vec();
        if let Some(ref agc) = self.agc {
            agc.process(&mut data);
        }

        let speech = if let Some(ref mut vad) = self.vad {
            vad.feed(&data)
        } else {
            data
        };

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
        if let Some(ref mut v) = self.vad {
            let tail = v.finish();
            if !tail.is_empty() {
                self.get_encoder()?.feed(&tail)?;
            }
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
        self.vad.as_mut().map(VadProcessor::reset);
        self.encoder = None;
    }
}
