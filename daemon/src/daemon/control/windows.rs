use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use anyhow::Context;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
};

use crate::APP_NAME;

use super::{ControlEvent, SignalState};

const OPEN_RETRIES: usize = 50;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(10);

pub(crate) fn toggle(pidfile: &Path) -> anyhow::Result<()> {
    let name = event_name(pidfile);
    let mut last_error = None;

    for attempt in 0..OPEN_RETRIES {
        let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if !handle.is_null() {
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            if unsafe { SetEvent(as_handle(&handle)) } != 0 {
                return Ok(());
            }
            return Err(std::io::Error::last_os_error()).context("failed to signal running daemon");
        }

        last_error = Some(std::io::Error::last_os_error());
        if attempt + 1 < OPEN_RETRIES {
            thread::sleep(OPEN_RETRY_DELAY);
        }
    }

    Err(last_error.unwrap())
        .with_context(|| format!("failed to find daemon IPC event for {}", pidfile.display()))
}

pub(super) fn install(pidfile: &Path) -> anyhow::Result<SignalState> {
    let toggle_event = create_event(pidfile)?;
    let (state, tx) = SignalState::with_channel();
    let shutdown = state.shutdown.clone();
    let stopper = state.stopper.clone();
    let shutdown_tx = tx.clone();

    ctrlc::set_handler(move || {
        shutdown.store(true, Ordering::SeqCst);
        if let Some(stopper) = stopper.lock().unwrap().clone() {
            stopper.stop();
        }
        let _ = shutdown_tx.send(ControlEvent::Shutdown);
    })
    .context("failed to install Ctrl+C handler")?;

    let stopper = state.stopper.clone();
    std::thread::spawn(move || loop {
        if let Err(error) = wait(&toggle_event) {
            eprintln!("daemon IPC listener failed: {error:#}");
            break;
        }

        if let Some(stopper) = stopper.lock().unwrap().clone() {
            stopper.stop();
        } else if tx.send(ControlEvent::Toggle).is_err() {
            break;
        }
    });

    Ok(state)
}

fn create_event(pidfile: &Path) -> anyhow::Result<OwnedHandle> {
    let name = event_name(pidfile);
    let handle = unsafe { CreateEventW(ptr::null(), 0, 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error()).context("failed to create daemon IPC event");
    }

    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    if already_exists {
        anyhow::bail!("daemon IPC event already exists for {}", pidfile.display());
    }

    Ok(handle)
}

fn wait(event: &OwnedHandle) -> anyhow::Result<()> {
    let result = unsafe { WaitForSingleObject(as_handle(event), INFINITE) };
    if result == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()).context("failed waiting for daemon IPC event")
    }
}

fn event_name(pidfile: &Path) -> Vec<u16> {
    let path = pidfile
        .canonicalize()
        .unwrap_or_else(|_| pidfile.to_path_buf());
    let mut hash = 0xcbf29ce484222325_u64;
    for unit in path.to_string_lossy().to_lowercase().encode_utf16() {
        hash ^= u64::from(unit);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!(r"Local\{APP_NAME}-control-{hash:016x}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

fn as_handle(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle() as HANDLE
}

#[cfg(test)]
mod tests {
    use crate::pidfile::{AcquireResult, PidFile};

    #[test]
    fn second_invocation_starts_idle_daemon() {
        let pid = std::process::id();
        let pidfile_path = std::env::temp_dir().join(format!("barkd-ipc-test-{pid}.pid"));
        let first = match PidFile::acquire(pidfile_path.clone()).unwrap() {
            AcquireResult::Acquired(pidfile) => pidfile,
            AcquireResult::AlreadyRunning => panic!("test pidfile is unexpectedly locked"),
        };
        let signals = super::install(&pidfile_path).unwrap();
        match PidFile::acquire(pidfile_path.clone()).unwrap() {
            AcquireResult::AlreadyRunning => {}
            AcquireResult::Acquired(_) => panic!("second invocation acquired the pidfile"),
        }

        super::toggle(&pidfile_path).unwrap();

        assert!(signals.wait_for_toggle().unwrap());
        drop(first);
        assert!(!pidfile_path.exists());
    }
}
