use std::fs;
use std::path::Path;

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

pub fn write(path: &Path, state: State) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, state.as_str())
        .with_context(|| format!("failed to write indicator: {}", path.display()))
}

pub fn clear(path: &Path) {
    let _ = fs::remove_file(path);
}
