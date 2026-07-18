use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use signal_hook::consts::signal::{SIGINT, SIGTERM, SIGUSR1};
use signal_hook::iterator::Signals;

use crate::audio::recorder::Stopper;

use super::{ControlEvent, SignalState};

pub(crate) fn toggle(pidfile: &Path) -> anyhow::Result<()> {
    let pid = std::fs::read_to_string(pidfile)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("daemon not running (no pidfile: {})", pidfile.display()))?;
    let pid: u32 = pid
        .trim()
        .parse()
        .with_context(|| format!("invalid pidfile: {}", pidfile.display()))?;
    if pid == 0 {
        anyhow::bail!("invalid pid in pidfile: {}", pidfile.display());
    }

    let pid = libc::pid_t::try_from(pid).context("PID is too large for this platform")?;
    let rc = unsafe { libc::kill(pid, SIGUSR1) };
    if rc != 0 {
        Err(std::io::Error::last_os_error()).context("failed to send SIGUSR1")
    } else {
        Ok(())
    }
}

pub(super) fn install(_pidfile: &Path) -> anyhow::Result<SignalState> {
    let (state, tx) = SignalState::with_channel();
    let thread_state = SignalThreadState {
        shutdown: state.shutdown.clone(),
        stopper: state.stopper.clone(),
        tx,
    };

    let mut signals = Signals::new([SIGUSR1, SIGINT, SIGTERM])?;
    std::thread::spawn(move || {
        for signal in signals.forever() {
            match signal {
                SIGUSR1 => {
                    if let Some(stopper) = thread_state.current_stopper() {
                        stopper.stop();
                    } else if thread_state.tx.send(ControlEvent::Toggle).is_err() {
                        break;
                    }
                }
                SIGINT | SIGTERM => {
                    thread_state.shutdown.store(true, Ordering::SeqCst);
                    if let Some(stopper) = thread_state.current_stopper() {
                        stopper.stop();
                    }
                    let _ = thread_state.tx.send(ControlEvent::Shutdown);
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(state)
}

struct SignalThreadState {
    shutdown: Arc<AtomicBool>,
    stopper: Arc<Mutex<Option<Stopper>>>,
    tx: Sender<ControlEvent>,
}

impl SignalThreadState {
    fn current_stopper(&self) -> Option<Stopper> {
        self.stopper.lock().unwrap().clone()
    }
}
