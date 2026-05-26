use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Context};
use fs2::FileExt;

use crate::APP_NAME;

pub struct PidFile {
    _file: File,
    path: PathBuf,
}

impl PidFile {
    pub fn acquire(path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create runtime dir: {}", parent.display()))?;
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .with_context(|| format!("failed to open pidfile: {}", path.display()))?;

        if file.try_lock_exclusive().is_err() {
            bail!("another {APP_NAME} instance is already running");
        }

        file.set_len(0)?;
        writeln!(file, "{}", std::process::id())?;
        file.sync_data().ok();

        Ok(Self { _file: file, path })
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
