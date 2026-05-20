mod opus;

use thiserror::Error;
pub use opus::OpusEncoder;

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("opus error: {0}")]
    Opus(#[from] ::opus::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
