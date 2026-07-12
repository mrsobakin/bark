use std::sync::mpsc::channel;

use anyhow::{anyhow, Context};
use bark_core::SAMPLE_RATE;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig, StreamError};

pub fn play(samples: &[i16]) -> anyhow::Result<()> {
    if samples.is_empty() {
        eprintln!("No audio to play");
        return Ok(());
    }

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no speaker found; check your audio output devices")?;
    eprintln!("Output device: {}", device.name()?);

    let supported = device.default_output_config()?;
    let sample_format = supported.sample_format();
    let config = supported.config();
    let audio = resample_mono_to_output(samples, &config);
    let total = audio.len();
    let mut pos = 0usize;
    let (done_tx, done_rx) = channel::<Result<(), String>>();
    let error_tx = done_tx.clone();
    let mut done_tx = Some(done_tx);

    let err_fn = move |err: StreamError| {
        let _ = error_tx.send(Err(err.to_string()));
    };
    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            &config,
            move |out: &mut [f32], _| {
                write_output(out, &audio, &mut pos, total, &mut done_tx, |x| x)
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_output_stream(
            &config,
            move |out: &mut [i16], _| {
                write_output(out, &audio, &mut pos, total, &mut done_tx, f32_to_i16)
            },
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_output_stream(
            &config,
            move |out: &mut [u16], _| {
                write_output(out, &audio, &mut pos, total, &mut done_tx, f32_to_u16)
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("unsupported output sample format: {other:?}")),
    };

    stream.play().context("failed to start playback stream")?;
    done_rx
        .recv()
        .context("playback stream stopped unexpectedly")?
        .map_err(anyhow::Error::msg)?;
    drop(stream);
    Ok(())
}

fn write_output<T>(
    out: &mut [T],
    audio: &[f32],
    pos: &mut usize,
    total: usize,
    done_tx: &mut Option<std::sync::mpsc::Sender<Result<(), String>>>,
    convert: impl Fn(f32) -> T,
) {
    for sample in out {
        if *pos < total {
            *sample = convert(audio[*pos]);
            *pos += 1;
        } else {
            *sample = convert(0.0);
            if let Some(tx) = done_tx.take() {
                let _ = tx.send(Ok(()));
            }
        }
    }
}

fn resample_mono_to_output(samples: &[i16], config: &StreamConfig) -> Vec<f32> {
    let channels = config.channels.max(1) as usize;
    let out_rate = config.sample_rate.0 as f32;
    let ratio = SAMPLE_RATE as f32 / out_rate;
    let out_frames = ((samples.len() as f32) / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_frames * channels);

    for frame in 0..out_frames {
        let src = frame as f32 * ratio;
        let i = src.floor() as usize;
        let frac = src - i as f32;
        let a = sample_to_f32(samples.get(i).copied().unwrap_or(0));
        let b = sample_to_f32(samples.get(i + 1).copied().unwrap_or(0));
        let sample = a + (b - a) * frac;
        for _ in 0..channels {
            out.push(sample);
        }
    }

    out
}

fn sample_to_f32(sample: i16) -> f32 {
    sample as f32 / 32768.0
}

fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

fn f32_to_u16(sample: f32) -> u16 {
    ((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32).round() as u16
}
