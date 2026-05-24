#![feature(slice_as_array)]

mod audio;
mod bark;
mod config;
mod engine;
mod post;
mod pre;
mod util;

#[derive(Debug, thiserror::Error)]
pub enum BarkError {
    #[error("Ogg/Opus muxing error: {0}")]
    Encoding(#[from] crate::audio::EncodeError),

    #[error("VAD error: {0}")]
    Vad(#[from] crate::pre::vad::VadError),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Transcription failed: {0}")]
    Transcription(#[from] crate::engine::TranscriptionError),

    #[error("Configuration error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, BarkError>;

pub use bark::*;
pub use config::*;
pub use engine::*;
pub use post::*;
pub use pre::*;
