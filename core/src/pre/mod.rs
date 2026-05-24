mod agc;
pub mod vad;

pub use agc::Agc;
pub use vad::{SileroVad, VadError, VadProcessor, VAD_FRAME_SAMPLES};
