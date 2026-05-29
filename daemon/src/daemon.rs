use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use bark_core::Bark;
use signal_hook::consts::signal::{SIGINT, SIGTERM, SIGUSR1};
use signal_hook::iterator::Signals;

use crate::audio::recorder::{CallbackAction, Recorder, StopReason, Stopper};
use crate::config::Config;
use crate::indicator::{self, State};
use crate::pidfile::PidFile;
use crate::typer;
use crate::APP_NAME;

pub fn toggle(pidfile: &Path) -> anyhow::Result<()> {
    let pid = fs::read_to_string(pidfile)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("daemon not running (no pidfile: {})", pidfile.display()))?;
    let pid: libc::pid_t = pid
        .trim()
        .parse()
        .with_context(|| format!("invalid pidfile: {}", pidfile.display()))?;

    let exists = unsafe { libc::kill(pid, 0) };
    if exists != 0 {
        anyhow::bail!("daemon not running (stale pidfile: {})", pidfile.display());
    }

    let rc = unsafe { libc::kill(pid, SIGUSR1) };
    if rc != 0 {
        Err(std::io::Error::last_os_error()).context("failed to send SIGUSR1")
    } else {
        Ok(())
    }
}

pub fn run(config: Config) -> anyhow::Result<()> {
    crate::config::validate_pipeline(&config.pipeline)?;

    let pidfile_path = config.daemon.pidfile.clone();
    let indicator_path = config.daemon.indicator_file.clone();
    let _pidfile = PidFile::acquire(pidfile_path.clone())?;

    let signals = SignalState::install()?;

    eprintln!("{APP_NAME} v{} ready", env!("CARGO_PKG_VERSION"));
    eprintln!("PID      : {}", std::process::id());
    eprintln!("Pidfile  : {}", pidfile_path.display());
    eprintln!("Indicator: {}", indicator_path.display());
    eprintln!("Typer    : {}", config.daemon.typer.join(" "));
    eprintln!("Model    : {}", config.pipeline.engine.model);
    eprintln!(
        "Language : {}",
        config
            .pipeline
            .engine
            .language
            .as_deref()
            .unwrap_or("auto-detect")
    );

    while !signals.shutdown() {
        if !signals.wait_for_toggle()? {
            break;
        }

        indicator::write(&indicator_path, State::Recording)?;
        let mut bark = Bark::new(config.pipeline.clone())?;
        let (recorder, stopper) = Recorder::new();
        let audio_error = Arc::new(Mutex::new(None::<String>));
        signals.set_stopper(Some(stopper));

        let stop_reason = recorder.record(config.daemon.timeout, |audio| {
            match bark.push_audio(audio) {
                Ok(()) => CallbackAction::Continue,
                Err(e) => {
                    *audio_error.lock().unwrap() = Some(e.to_string());
                    CallbackAction::Stop
                }
            }
        });

        signals.set_stopper(None);
        let stop_reason = match stop_reason {
            Ok(stop_reason) => stop_reason,
            Err(err) => {
                indicator::clear(&indicator_path);
                return Err(err);
            }
        };

        if signals.shutdown() {
            indicator::clear(&indicator_path);
            break;
        }

        if let Some(err) = audio_error.lock().unwrap().take() {
            indicator::clear(&indicator_path);
            eprintln!("Audio processing failed: {err}");
            continue;
        }

        indicator::write(&indicator_path, State::Transcribing)?;
        if let Err(err) = finish(&config, bark, stop_reason) {
            eprintln!("Transcription failed: {err:#}");
        }

        indicator::clear(&indicator_path);
    }

    indicator::clear(&indicator_path);
    Ok(())
}

fn finish(config: &Config, mut bark: Bark, stop_reason: StopReason) -> anyhow::Result<()> {
    if stop_reason == StopReason::Timeout {
        eprintln!(
            "Recording cancelled: exceeded {:.1}s timeout",
            config.daemon.timeout.as_secs_f64()
        );
        return Ok(());
    }

    let text = bark.finalize()?;
    let text = text.trim().replace('\n', " ");

    if text.is_empty() {
        eprintln!("No speech detected");
        return Ok(());
    }

    eprintln!("Final transcription: {text:?}");
    typer::type_text(&config.daemon.typer, &text)
}

enum ControlEvent {
    Toggle,
    Shutdown,
}

struct SignalState {
    shutdown: Arc<AtomicBool>,
    stopper: Arc<Mutex<Option<Stopper>>>,
    rx: Receiver<ControlEvent>,
}

impl SignalState {
    fn install() -> anyhow::Result<Self> {
        let (tx, rx) = channel();
        let state = Self {
            shutdown: Arc::new(AtomicBool::new(false)),
            stopper: Arc::new(Mutex::new(None)),
            rx,
        };
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

    fn shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    fn set_stopper(&self, stopper: Option<Stopper>) {
        *self.stopper.lock().unwrap() = stopper;
    }

    fn wait_for_toggle(&self) -> anyhow::Result<bool> {
        match self.rx.recv().context("signal thread exited")? {
            ControlEvent::Toggle => Ok(true),
            ControlEvent::Shutdown => Ok(false),
        }
    }
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
