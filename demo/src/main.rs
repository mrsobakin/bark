//! bark-demo – minimal CLI that records from the microphone until Ctrl+C,
//! then transcribes the audio via bark-core.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn main() -> anyhow::Result<()> {
    // -- configuration (from environment) -----------------------------------
    let api_key =
        std::env::var("GROQ_API_KEY").context("GROQ_API_KEY environment variable must be set")?;

    let endpoint = std::env::var("BARK_ENDPOINT")
        .unwrap_or_else(|_| "https://api.groq.com/openai/v1/audio/transcriptions".into());
    let model = std::env::var("BARK_MODEL").unwrap_or_else(|_| "whisper-large-v3-turbo".into());

    let bark_config = bark_core::BarkConfig {
        engine: bark_core::EngineConfig {
            api_key,
            endpoint,
            model,
            language: None,
            prompt: None,
        },
        pre: bark_core::PreConfig {
            agc: None,
            vad: None,
        },
        post: bark_core::PostConfig {},
    };

    // -- audio device setup -------------------------------------------------
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no microphone found – check your audio input devices")?;
    let supported = device.default_input_config()?;

    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;

    eprintln!("Device     : {}", device.name()?);
    eprintln!("Sample rate: {} Hz", sample_rate);
    eprintln!("Channels   : {}", channels);
    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    eprintln!("Recording… press Ctrl+C to stop.");
    eprintln!();

    // -- shared state -------------------------------------------------------
    let audio: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let running = Arc::new(AtomicBool::new(true));

    // -- build the capture stream -------------------------------------------
    let stream = build_stream(
        &device,
        &supported.config(),
        &audio,
        &running,
        channels,
        supported.sample_format(),
    )?;

    // -- Ctrl+C handler -----------------------------------------------------
    ctrlc::set_handler({
        let running = running.clone();
        move || {
            running.store(false, Ordering::SeqCst);
        }
    })
    .context("failed to install Ctrl+C handler")?;

    // -- wait until user hits Ctrl+C ----------------------------------------
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(stream); // ensure the capture callback won't run after this point

    let samples_f32 = audio.lock().unwrap().clone();
    if samples_f32.is_empty() {
        eprintln!("No audio captured.");
        return Ok(());
    }

    eprintln!(
        "Transcribing {:.1} seconds of audio…",
        samples_f32.len() as f64 / sample_rate as f64
    );

    // -- convert to 16 kHz mono i16 ----------------------------------------
    let samples_i16 = convert_to_mono_i16_16khz(&samples_f32, sample_rate);

    // -- run the bark pipeline ----------------------------------------------
    let mut bark = bark_core::Bark::new(bark_config)?;
    bark.push_audio(&samples_i16);
    let text = bark.finalize()?;

    // -- output -------------------------------------------------------------
    if text.is_empty() {
        println!("(no speech detected)");
    } else {
        println!("{}", text);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Build an input stream, dispatching on sample format
// ---------------------------------------------------------------------------

fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    audio: &Arc<Mutex<Vec<f32>>>,
    running: &Arc<AtomicBool>,
    channels: usize,
    fmt: cpal::SampleFormat,
) -> anyhow::Result<cpal::Stream> {
    match fmt {
        cpal::SampleFormat::F32 => build_stream_f32(device, config, audio, running, channels),
        cpal::SampleFormat::I16 => build_stream_i16(device, config, audio, running, channels),
        cpal::SampleFormat::I32 => build_stream_i32(device, config, audio, running, channels),
        cpal::SampleFormat::U16 => build_stream_u16(device, config, audio, running, channels),
        other => anyhow::bail!("unsupported sample format: {other:?}"),
    }
}

macro_rules! make_stream_fn {
    ($name:ident, $ty:ty, $convert:expr) => {
        fn $name(
            device: &cpal::Device,
            config: &cpal::StreamConfig,
            audio: &Arc<Mutex<Vec<f32>>>,
            running: &Arc<AtomicBool>,
            channels: usize,
        ) -> anyhow::Result<cpal::Stream> {
            let audio = audio.clone();
            let running = running.clone();
            let err_fn = |err| eprintln!("[stream error] {err}");

            let stream = device
                .build_input_stream(
                    config,
                    move |data: &[$ty], _: &cpal::InputCallbackInfo| {
                        if !running.load(Ordering::SeqCst) {
                            return;
                        }
                        let mut buf = audio.lock().unwrap();
                        for (i, &sample) in data.iter().enumerate() {
                            if i % channels == 0 {
                                buf.push($convert(sample));
                            }
                        }
                    },
                    err_fn,
                    None,
                )
                .context("failed to build input stream")?;

            stream.play().context("failed to start capture stream")?;
            Ok(stream)
        }
    };
}

make_stream_fn!(build_stream_f32, f32, |s: f32| s);
make_stream_fn!(build_stream_i16, i16, |s: i16| s as f32 / 32768.0);
make_stream_fn!(build_stream_i32, i32, |s: i32| s as f32 / 2147483648.0);
make_stream_fn!(build_stream_u16, u16, |s: u16| (s as f32 - 32768.0)
    / 32768.0);

// ---------------------------------------------------------------------------
// Convert a mono f32 buffer at an arbitrary sample rate to 16 kHz mono i16
// ---------------------------------------------------------------------------

fn convert_to_mono_i16_16khz(samples: &[f32], sample_rate: u32) -> Vec<i16> {
    const TARGET: u32 = 16_000;

    if sample_rate == TARGET {
        return samples.iter().map(|&s| (s * 32768.0) as i16).collect();
    }

    // Linear-interpolation resampling.
    let ratio = sample_rate as f64 / TARGET as f64;
    let out_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);

    let mut phase = 0.0_f64;
    while (phase as usize) < samples.len().saturating_sub(1) {
        let i = phase as usize;
        let frac = phase - i as f64;
        let s = samples[i] * (1.0 - frac as f32) + samples[i + 1] * frac as f32;
        out.push((s * 32768.0) as i16);
        phase += ratio;
    }

    out
}
