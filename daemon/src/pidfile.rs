use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::Context;
use fs2::FileExt;

use crate::APP_NAME;

pub struct PidFile {
    _file: File,
    path: PathBuf,
}

pub enum AcquireResult {
    Acquired(PidFile),
    AlreadyRunning(libc::pid_t),
}

impl PidFile {
    pub fn acquire(path: PathBuf) -> anyhow::Result<AcquireResult> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create runtime dir: {}", parent.display()))?;
        }

        // Open without truncate so we can read the existing pid if the lock fails.
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open pidfile: {}", path.display()))?;

        if file.try_lock_exclusive().is_err() {
            let mut buf = String::new();
            file.read_to_string(&mut buf).ok();
            let pid = buf.trim().parse::<libc::pid_t>().unwrap_or(0);
            if pid > 0 {
                return Ok(AcquireResult::AlreadyRunning(pid));
            }
            anyhow::bail!("another {APP_NAME} instance is starting; please retry");
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
