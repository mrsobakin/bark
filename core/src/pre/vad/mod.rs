use crate::config::VadConfig;
use crate::util::chunker::Chunker;
use crate::SAMPLE_RATE;
use thiserror::Error;

mod fsm;
mod silero;

use fsm::VadFSM;
pub use silero::{SileroVad, VAD_FRAME_SAMPLES};

#[derive(Error, Debug)]
#[error("{0}")]
pub struct VadError(String);

pub struct VadProcessor {
    vad: SileroVad,
    fsm: VadFSM,
    chunker: Chunker<i16, VAD_FRAME_SAMPLES>,
    threshold: f32,
}

impl VadProcessor {
    pub fn new(config: &VadConfig) -> Result<Self, VadError> {
        let silero = SileroVad::load()?;

        Ok(Self {
            vad: silero,
            fsm: VadFSM::new(config),
            chunker: Chunker::new(),
            threshold: config.threshold,
        })
    }

    pub fn feed(&mut self, audio: &[i16]) -> Result<Vec<i16>, VadError> {
        let mut result = Vec::new();

        self.chunker.feed(audio, |f| -> Result<(), VadError> {
            let is_speech = self.vad.is_speech(f, self.threshold)?;
            result.extend_from_slice(&self.fsm.process(is_speech, f));
            Ok(())
        })?;

        Ok(result)
    }

    pub fn finish(&mut self) -> Result<Vec<i16>, VadError> {
        let mut result = vec![];

        self.chunker.finish(|f| -> Result<(), VadError> {
            let is_speech = self.vad.is_speech(f, self.threshold)?;
            result = self.fsm.process(is_speech, f);
            Ok(())
        })?;

        Ok(result)
    }

    pub fn reset(&mut self) {
        self.vad.reset();
        self.fsm.reset();
        self.chunker.reset();
    }
}
