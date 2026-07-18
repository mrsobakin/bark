use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use anyhow::Context;

use crate::audio::recorder::Stopper;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::toggle;
#[cfg(windows)]
pub(crate) use windows::toggle;

enum ControlEvent {
    Toggle,
    Shutdown,
}

pub(super) struct SignalState {
    shutdown: Arc<AtomicBool>,
    stopper: Arc<Mutex<Option<Stopper>>>,
    rx: Receiver<ControlEvent>,
}

impl SignalState {
    pub(super) fn install(pidfile: &Path) -> anyhow::Result<Self> {
        #[cfg(unix)]
        return unix::install(pidfile);

        #[cfg(windows)]
        return windows::install(pidfile);
    }

    fn with_channel() -> (Self, Sender<ControlEvent>) {
        let (tx, rx) = channel();
        let state = Self {
            shutdown: Arc::new(AtomicBool::new(false)),
            stopper: Arc::new(Mutex::new(None)),
            rx,
        };
        (state, tx)
    }

    pub(super) fn shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    pub(super) fn install_stopper(&self, stopper: Stopper) -> bool {
        let mut current = self.stopper.lock().unwrap();
        *current = Some(stopper);

        if self.shutdown() {
            current.take().unwrap().stop();
            false
        } else {
            match self.rx.try_recv() {
                Ok(ControlEvent::Toggle) => current.as_ref().unwrap().stop(),
                Ok(ControlEvent::Shutdown) => {
                    current.take().unwrap().stop();
                    return false;
                }
                Err(_) => {}
            }
            true
        }
    }

    pub(super) fn set_stopper(&self, stopper: Option<Stopper>) {
        *self.stopper.lock().unwrap() = stopper;
    }

    pub(super) fn wait_for_toggle(&self) -> anyhow::Result<bool> {
        match self.rx.recv().context("signal thread exited")? {
            ControlEvent::Toggle => Ok(true),
            ControlEvent::Shutdown => Ok(false),
        }
    }
}
