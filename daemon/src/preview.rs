use std::sync::{Arc, Mutex};

use anyhow::Context;
use bark_core::Preprocessor;
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::audio::playback;
use crate::audio::recorder::{CallbackAction, Recorder, Stopper};
use crate::config::Config;

pub fn run(config: Config) -> anyhow::Result<()> {
    let mut preprocessor = Preprocessor::new(&config.pipeline.pre)?;
    let (recorder, stopper) = Recorder::new();
    let _signals = StopSignals::install(stopper)?;
    let error = Arc::new(Mutex::new(None::<String>));
    let mut processed = Vec::new();

    eprintln!("Recording preview. Press Ctrl+C to stop.");
    let record_result = recorder.record(config.daemon.timeout, |audio| {
        match preprocessor.process(audio) {
            Ok(mut audio) => {
                processed.append(&mut audio);
                CallbackAction::Continue
            }
            Err(err) => {
                *error.lock().unwrap() = Some(err.to_string());
                CallbackAction::Stop
            }
        }
    });

    record_result.context("preview recording failed")?;

    if let Some(err) = error.lock().unwrap().take() {
        anyhow::bail!("audio preprocessing failed: {err}");
    }

    processed.extend(preprocessor.finish()?);

    eprintln!("Playing processed preview.");
    playback::play(&processed)
}

struct StopSignals;

impl StopSignals {
    fn install(stopper: Stopper) -> anyhow::Result<Self> {
        let mut signals = Signals::new([SIGINT, SIGTERM])?;
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                stopper.stop();
            }
        });
        Ok(Self)
    }
}
