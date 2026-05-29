use crate::config::PreConfig;
use crate::pre::vad::VadProcessor;
use crate::pre::Agc;
use crate::Result;

pub struct Preprocessor {
    agc: Option<Agc>,
    vad: Option<VadProcessor>,
}

impl Preprocessor {
    pub fn new(config: &PreConfig) -> Result<Self> {
        Ok(Self {
            agc: config.agc.as_ref().map(Agc::new),
            vad: config.vad.as_ref().map(VadProcessor::new).transpose()?,
        })
    }

    pub fn process(&mut self, frames: &[i16]) -> Result<Vec<i16>> {
        let mut data = frames.to_vec();
        if let Some(ref mut agc) = self.agc {
            agc.process(&mut data);
        }

        if let Some(ref mut vad) = self.vad {
            Ok(vad.feed(&data)?)
        } else {
            Ok(data)
        }
    }

    pub fn finish(&mut self) -> Result<Vec<i16>> {
        if let Some(ref mut vad) = self.vad {
            Ok(vad.finish()?)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn reset(&mut self) {
        self.vad.as_mut().map(VadProcessor::reset);
        self.agc.as_mut().map(Agc::reset);
    }
}
