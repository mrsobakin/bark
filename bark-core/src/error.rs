/// Errors that can occur during the Bark pipeline.
#[derive(Debug, thiserror::Error)]
pub enum BarkError {
    #[error("VAD error: {0}")]
    Vad(String),

    #[error("Opus encoding error: {0}")]
    Opus(#[from] opus::Error),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Transcription failed: {0}")]
    Transcription(String),

    #[error("No speech detected in audio")]
    NoSpeech,

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("ONNX Runtime error: {0}")]
    OnnxRuntime(String),
}

impl BarkError {
    /// True when no speech was detected ( callers may treat this as a soft
    /// failure rather than a hard error).
    pub fn is_no_speech(&self) -> bool {
        matches!(self, Self::NoSpeech)
    }
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, BarkError>;