mod whisper;

use thiserror::Error;
pub use whisper::WhisperClient;

#[derive(Error, Debug)]
#[error("{0}")]
pub struct TranscriptionError(String);

impl From<reqwest::Error> for TranscriptionError {
    fn from(value: reqwest::Error) -> Self {
        Self(value.to_string())
    }
}

pub trait TranscriptionEngine {
    fn transcribe(&self, audio: &[u8]) -> Result<String, TranscriptionError>;
}
