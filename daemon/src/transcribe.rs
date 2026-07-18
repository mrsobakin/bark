use std::sync::{Arc, Mutex};

use anyhow::Context;
use bark_core::{Bark, Preprocessor};
#[cfg(unix)]
use signal_hook::consts::signal::{SIGINT, SIGTERM};
#[cfg(unix)]
use signal_hook::iterator::Signals;

use crate::audio::playback;
use crate::audio::recorder::{CallbackAction, Recorder, StopReason, Stopper};
use crate::config::Config;
use crate::typer;

#[derive(Debug, clap::Args)]
#[command(group = clap::ArgGroup::new("output").args(&["preview", "type_it"]).multiple(false))]
pub struct TranscribeArgs {
    /// Play back processed audio instead of transcribing.
    #[arg(long, group = "output")]
    pub preview: bool,

    /// Type the transcription instead of printing it.
    #[arg(long = "type", group = "output")]
    pub type_it: bool,
}

pub fn run(config: Config, args: TranscribeArgs) -> anyhow::Result<()> {
    let (recorder, stopper) = Recorder::new();
    let _signals = StopSignals::install(stopper)?;

    if args.preview {
        run_preview(config, recorder)
    } else {
        run_transcribe(config, recorder, args.type_it)
    }
}

fn run_preview(config: Config, recorder: Recorder) -> anyhow::Result<()> {
    let mut preprocessor = Preprocessor::new(&config.pipeline.pre)?;
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

fn run_transcribe(config: Config, recorder: Recorder, type_it: bool) -> anyhow::Result<()> {
    crate::config::validate_pipeline(&config.pipeline)?;

    let mut bark = Bark::new(config.pipeline)?;
    let audio_error = Arc::new(Mutex::new(None::<String>));

    eprintln!("Recording. Press Ctrl+C to stop.");
    let stop_reason = recorder.record(config.daemon.timeout, |audio| {
        match bark.push_audio(audio) {
            Ok(()) => CallbackAction::Continue,
            Err(e) => {
                *audio_error.lock().unwrap() = Some(e.to_string());
                CallbackAction::Stop
            }
        }
    });

    let stop_reason = stop_reason.context("recording failed")?;

    if let Some(err) = audio_error.lock().unwrap().take() {
        anyhow::bail!("audio processing failed: {err}");
    }

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

    if type_it {
        typer::type_text(&config.daemon.typer, &text)?;
    } else {
        println!("{text}");
    }
    Ok(())
}

struct StopSignals;

impl StopSignals {
    #[cfg(unix)]
    fn install(stopper: Stopper) -> anyhow::Result<Self> {
        let mut signals = Signals::new([SIGINT, SIGTERM])?;
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                stopper.stop();
            }
        });
        Ok(Self)
    }

    #[cfg(windows)]
    fn install(stopper: Stopper) -> anyhow::Result<Self> {
        ctrlc::set_handler(move || stopper.stop()).context("failed to install Ctrl+C handler")?;
        Ok(Self)
    }
}
