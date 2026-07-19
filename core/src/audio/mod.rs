mod opus;
#[cfg(feature = "resampler")]
mod resampler;

pub use opus::OpusEncoder;
#[cfg(feature = "resampler")]
pub use resampler::Resampler;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("opus error: {0}")]
    Opus(#[from] ::opus::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
