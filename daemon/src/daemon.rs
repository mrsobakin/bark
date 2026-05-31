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
use crate::indicator::{Indicator, State};
use crate::pidfile::{PidFile, AcquireResult};
use crate::typer;
use crate::APP_NAME;

pub fn run(config: Config, oneshot: bool) -> anyhow::Result<()> {
    let pidfile_path = config.daemon.pidfile.clone();

    match PidFile::acquire(pidfile_path)? {
        AcquireResult::AlreadyRunning(pid) => toggle_pid(pid),
        AcquireResult::Acquired(_pidfile) => {
            let daemon = Daemon::new(config)?;
            if oneshot {
                daemon.run_once()
            } else {
                daemon.run_loop()
            }
        }
    }
}

pub fn toggle(pidfile: &Path) -> anyhow::Result<()> {
    let pid = fs::read_to_string(pidfile)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("daemon not running (no pidfile: {})", pidfile.display()))?;
    let pid: libc::pid_t = pid
        .trim()
        .parse()
        .with_context(|| format!("invalid pidfile: {}", pidfile.display()))?;
    if pid <= 0 {
        anyhow::bail!("invalid pid in pidfile: {}", pidfile.display());
    }
    toggle_pid(pid)
}

fn toggle_pid(pid: libc::pid_t) -> anyhow::Result<()> {
    let rc = unsafe { libc::kill(pid, SIGUSR1) };
    if rc != 0 {
        Err(std::io::Error::last_os_error()).context("failed to send SIGUSR1")
    } else {
        Ok(())
    }
}

struct Daemon {
    config: Config,
    signals: SignalState,
}

impl Daemon {
    fn new(config: Config) -> anyhow::Result<Self> {
        crate::config::validate_pipeline(&config.pipeline)?;
        let signals = SignalState::install()?;
        Ok(Self { config, signals })
    }

    fn run_once(self) -> anyhow::Result<()> {
        let indicator = Indicator::new(&self.config.daemon.indicator_file);
        let (recorder, stopper) = Recorder::new();

        self.signals.set_stopper(Some(stopper));
        let _ = indicator.write(State::Recording);
        let result = self.record(recorder);

        self.signals.set_stopper(None);
        let _ = indicator.write(State::Transcribing);

        let (bark, stop_reason) = result?;
        self.finish(bark, stop_reason)
    }

    fn run_loop(self) -> anyhow::Result<()> {
        let indicator_path = &self.config.daemon.indicator_file;

        eprintln!("{APP_NAME} v{} ready", env!("CARGO_PKG_VERSION"));
        eprintln!("PID      : {}", std::process::id());
        eprintln!("Indicator: {}", indicator_path.display());
        eprintln!("Typer    : {}", self.config.daemon.typer.join(" "));
        eprintln!("Model    : {}", self.config.pipeline.engine.model);
        eprintln!(
            "Language : {}",
            self.config
                .pipeline
                .engine
                .language
                .as_deref()
                .unwrap_or("auto-detect")
        );

        while !self.signals.shutdown() {
            if !self.signals.wait_for_toggle()? {
                break;
            }

            let indicator = Indicator::new(&self.config.daemon.indicator_file);

            let _ = indicator.write(State::Recording);
            let (recorder, stopper) = Recorder::new();
            self.signals.set_stopper(Some(stopper));

            let result = self.record(recorder);
            self.signals.set_stopper(None);

            match result {
                Ok((bark, stop_reason)) => {
                    let _ = indicator.write(State::Transcribing);
                    if let Err(err) = self.finish(bark, stop_reason) {
                        eprintln!("Transcription failed: {err:#}");
                    }
                }
                Err(err) => {
                    eprintln!("{err:#}");
                }
            }
        }

        Ok(())
    }

    fn record(&self, recorder: Recorder) -> anyhow::Result<(Bark, StopReason)> {
        let mut bark = Bark::new(self.config.pipeline.clone())?;
        let audio_error = Arc::new(Mutex::new(None::<String>));

        let stop_reason = recorder.record(self.config.daemon.timeout, |audio| {
            match bark.push_audio(audio) {
                Ok(()) => CallbackAction::Continue,
                Err(e) => {
                    *audio_error.lock().unwrap() = Some(e.to_string());
                    CallbackAction::Stop
                }
            }
        }).context("recording failed")?;

        if let Some(err) = audio_error.lock().unwrap().take() {
            anyhow::bail!("audio processing failed: {err}");
        }

        Ok((bark, stop_reason))
    }

    fn finish(&self, mut bark: Bark, stop_reason: StopReason) -> anyhow::Result<()> {
        if stop_reason == StopReason::Timeout {
            eprintln!(
                "Recording cancelled: exceeded {:.1}s timeout",
                self.config.daemon.timeout.as_secs_f64()
            );
            return Ok(());
        }

        let text = bark.finalize()?;
        let text = text.trim();

        if text.is_empty() {
            eprintln!("No speech detected");
            return Ok(());
        }

        eprintln!("Final transcription: {text:?}");
        typer::type_text(&self.config.daemon.typer, &text)
    }
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
