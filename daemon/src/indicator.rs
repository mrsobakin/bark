use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

#[derive(Debug, Clone, Copy)]
pub enum State {
    Recording,
    Transcribing,
}

impl State {
    fn as_str(self) -> &'static str {
        match self {
            State::Recording => "recording",
            State::Transcribing => "transcribing",
        }
    }
}

pub struct Indicator {
    path: PathBuf,
}

impl Indicator {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    pub fn write(&self, state: State) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&self.path, state.as_str())
            .with_context(|| format!("failed to write indicator: {}", self.path.display()))
    }
}

impl Drop for Indicator {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
