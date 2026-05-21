mod opus;

pub use opus::OpusEncoder;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("opus error: {0}")]
    Opus(#[from] ::opus::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
