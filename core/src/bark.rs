use crate::config::BarkConfig;
use crate::engine::{OpenAIClient, TranscriptionEngine};
use crate::post;
use crate::pre::Preprocessor;
use crate::Result;

pub struct Bark {
    config: BarkConfig,
    engine: Option<Box<dyn TranscriptionEngine>>,
    preprocessor: Preprocessor,
}

impl Bark {
    pub fn new(config: BarkConfig) -> Result<Bark> {
        let preprocessor = Preprocessor::new(&config.pre)?;

        Ok(Self {
            config,
            engine: None,
            preprocessor,
        })
    }

    pub fn push_audio(&mut self, frames: &[i16]) -> Result<()> {
        let speech = self.preprocessor.process(frames)?;

        if !speech.is_empty() {
            self.get_engine()?.push_audio(&speech)?;
        }

        Ok(())
    }

    fn get_engine(&mut self) -> Result<&mut Box<dyn TranscriptionEngine>> {
        if self.engine.is_none() {
            self.engine = Some(Box::new(OpenAIClient::new(&self.config.engine)?));
        }
        Ok(self.engine.as_mut().unwrap())
    }

    pub fn finalize(&mut self) -> Result<String> {
        let tail = self.preprocessor.finish()?;
        if !tail.is_empty() {
            self.get_engine()?.push_audio(&tail)?;
        }

        let Some(engine) = self.engine.take() else {
            return Ok(String::new());
        };

        let text = engine.finalize()?;

        if text.is_empty() {
            return Ok(String::new());
        }

        let final_text = post::postprocess(&text, &self.config.post);
        Ok(final_text)
    }

    pub fn reset(&mut self) {
        self.preprocessor.reset();
        self.engine = None;
    }
}
