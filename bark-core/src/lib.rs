//! `bark-core` – speech-to-text pipeline library.
//!
//! The primary entry point is [`Bark`]: push raw audio frames, then finalise
//! to receive the transcribed text.

mod bark;
mod config;
mod error;
mod ogg_opus;
mod postprocessor;
mod preprocessor;
mod vad;
mod whisper;

pub use bark::Bark;
pub use config::{
    AgcConfig, BarkConfig, DeEmdasherConfig, EngineConfig, PostConfig, PreConfig, VadConfig,
};
pub use error::{BarkError, Result};
pub use vad::{EnergyVad, VadProcessor, VoiceDetector, VAD_FRAME_SAMPLES};

#[cfg(feature = "vad-silero")]
pub use vad::SileroVad;