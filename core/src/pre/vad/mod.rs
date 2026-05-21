use crate::config::VadConfig;
use crate::util::chunker::Chunker;
use thiserror::Error;

const SAMPLE_RATE: u32 = 16_000;

mod fsm;
mod silero;

use fsm::VadFSM;
use silero::{SileroVad, VAD_FRAME_SAMPLES};

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

    pub fn feed(&mut self, audio: &[i16]) -> Vec<i16> {
        let mut result = Vec::new();

        self.chunker.feed(audio, |f| {
            let is_speech = self.vad.is_speech(f, self.threshold);
            result.extend_from_slice(&self.fsm.process(is_speech, f));
        });

        result
    }

    pub fn finish(&mut self) -> Vec<i16> {
        let mut result = vec![];

        self.chunker.finish(|f| {
            let is_speech = self.vad.is_speech(f, self.threshold);
            result = self.fsm.process(is_speech, f);
        });

        result
    }

    pub fn reset(&mut self) {
        self.vad.reset();
        self.fsm.reset();
        self.chunker.reset();
    }
}
