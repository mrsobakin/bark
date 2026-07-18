use std::sync::{Arc, Mutex};

use anyhow::Context;
use bark_core::Bark;

use crate::audio::recorder::{CallbackAction, Recorder, StopReason};
use crate::config::Config;
use crate::indicator::{Indicator, State};
use crate::pidfile::{AcquireResult, PidFile};
use crate::typer;
use crate::APP_NAME;

mod control;

pub(crate) use control::toggle;
use control::SignalState;

pub fn run(config: Config, oneshot: bool) -> anyhow::Result<()> {
    let pidfile_path = config.daemon.pidfile.clone();

    match PidFile::acquire(pidfile_path.clone())? {
        AcquireResult::AlreadyRunning => toggle(&pidfile_path),
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

struct Daemon {
    config: Config,
    signals: SignalState,
}

impl Daemon {
    fn new(config: Config) -> anyhow::Result<Self> {
        crate::config::validate_pipeline(&config.pipeline)?;
        let signals = SignalState::install(&config.daemon.pidfile)?;
        Ok(Self { config, signals })
    }

    fn run_once(self) -> anyhow::Result<()> {
        let indicator = Indicator::new(&self.config.daemon.indicator_file);
        let (recorder, stopper) = Recorder::new();

        if !self.signals.install_stopper(stopper) {
            return Ok(());
        }
        let _ = indicator.write(State::Recording);
        let result = self.record(recorder);

        self.signals.set_stopper(None);
        if self.signals.shutdown() {
            return Ok(());
        }
        let _ = indicator.write(State::Transcribing);

        let (bark, stop_reason) = result?;
        self.finish(bark, stop_reason)
    }

    fn run_loop(self) -> anyhow::Result<()> {
        let indicator_path = &self.config.daemon.indicator_file;

        eprintln!("{APP_NAME} v{} ready", env!("CARGO_PKG_VERSION"));
        eprintln!("PID      : {}", std::process::id());
        eprintln!("Indicator: {}", indicator_path.display());
        let typer = if self.config.daemon.typer.is_empty() {
            "native".to_owned()
        } else {
            self.config.daemon.typer.join(" ")
        };
        eprintln!("Typer    : {typer}");
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

            let (recorder, stopper) = Recorder::new();
            if !self.signals.install_stopper(stopper) {
                break;
            }
            let _ = indicator.write(State::Recording);

            let result = self.record(recorder);
            self.signals.set_stopper(None);
            if self.signals.shutdown() {
                break;
            }

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

        let stop_reason = recorder
            .record(self.config.daemon.timeout, |audio| {
                match bark.push_audio(audio) {
                    Ok(()) => CallbackAction::Continue,
                    Err(e) => {
                        *audio_error.lock().unwrap() = Some(e.to_string());
                        CallbackAction::Stop
                    }
                }
            })
            .context("recording failed")?;

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
        typer::type_text(&self.config.daemon.typer, text)
    }
}
