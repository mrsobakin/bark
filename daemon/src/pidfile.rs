use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use fs2::FileExt;

pub struct PidFile {
    _file: File,
    path: PathBuf,
}

pub enum AcquireResult {
    Acquired(PidFile),
    AlreadyRunning,
}

impl PidFile {
    pub fn acquire(path: PathBuf) -> anyhow::Result<AcquireResult> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create runtime dir: {}", parent.display()))?;
        }

        // Do not truncate until after locking; another instance may own the file.
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open pidfile: {}", path.display()))?;

        if file.try_lock_exclusive().is_err() {
            return Ok(AcquireResult::AlreadyRunning);
        }

        file.set_len(0)?;
        writeln!(file, "{}", std::process::id())?;
        file.sync_data().ok();

        Ok(AcquireResult::Acquired(Self { _file: file, path }))
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
