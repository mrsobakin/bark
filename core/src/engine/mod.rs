mod openai;

pub use openai::OpenAIClient;
use thiserror::Error;

#[derive(Error, Debug)]
#[error("{0}")]
pub struct TranscriptionError(pub(crate) String);

impl From<ureq::Error> for TranscriptionError {
    fn from(value: ureq::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<std::io::Error> for TranscriptionError {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<crate::audio::EncodeError> for TranscriptionError {
    fn from(value: crate::audio::EncodeError) -> Self {
        Self(value.to_string())
    }
}

pub trait TranscriptionEngine {
    fn push_audio(&mut self, audio: &[i16]) -> Result<(), TranscriptionError>;
    fn finalize(self: Box<Self>) -> Result<String, TranscriptionError>;
}
