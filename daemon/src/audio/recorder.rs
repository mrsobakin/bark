use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use bark_core::{Resampler, SAMPLE_RATE};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    FromSample, Sample, SampleFormat, SampleRate, SizedSample, Stream, StreamConfig, StreamError,
    SupportedStreamConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Stopped,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackAction {
    Continue,
    Stop,
}

enum Event {
    Stop,
    Error(StreamError),
    Audio(Vec<i16>),
    AudioToResample(Vec<f32>),
}

fn make_stream(tx: Sender<Event>) -> anyhow::Result<(Stream, u32)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no microphone found; check your audio input devices")?;
    eprintln!("Device: {}", device.name()?);

    let supported = input_config(&device)?;
    let format = supported.sample_format();
    let rate = supported.sample_rate().0;
    let config = supported.config();
    eprintln!(
        "Input: {} channel(s), {} Hz, {:?}",
        config.channels, rate, format
    );

    let stream = if rate == SAMPLE_RATE {
        match format {
            SampleFormat::I16 => build_stream::<i16, _>(&device, &config, tx, audio_event::<i16>),
            SampleFormat::F32 => build_stream::<f32, _>(&device, &config, tx, audio_event::<f32>),
            SampleFormat::U16 => build_stream::<u16, _>(&device, &config, tx, audio_event::<u16>),
            other => return Err(anyhow!("unsupported input sample format: {other:?}")),
        }
    } else {
        match format {
            SampleFormat::I16 => {
                build_stream::<i16, _>(&device, &config, tx, resample_event::<i16>)
            }
            SampleFormat::F32 => {
                build_stream::<f32, _>(&device, &config, tx, resample_event::<f32>)
            }
            SampleFormat::U16 => {
                build_stream::<u16, _>(&device, &config, tx, resample_event::<u16>)
            }
            other => return Err(anyhow!("unsupported input sample format: {other:?}")),
        }
    }?;

    Ok((stream, rate))
}

fn input_config(device: &cpal::Device) -> anyhow::Result<SupportedStreamConfig> {
    let at_target_rate = device
        .supported_input_configs()?
        .filter(|config| {
            matches!(
                config.sample_format(),
                SampleFormat::I16 | SampleFormat::F32 | SampleFormat::U16
            )
        })
        .filter_map(|config| config.try_with_sample_rate(SampleRate(SAMPLE_RATE)))
        .min_by_key(|config| {
            (
                config.sample_format() != SampleFormat::I16,
                config.channels() != 1,
                config.channels(),
            )
        });

    at_target_rate
        .map_or_else(|| device.default_input_config(), Ok)
        .context("input device has no default stream configuration")
}

fn build_stream<T, C>(
    device: &cpal::Device,
    config: &StreamConfig,
    tx: Sender<Event>,
    convert: C,
) -> Result<Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    C: Fn(&[T], usize) -> Event + Send + 'static,
{
    let channels = usize::from(config.channels.max(1));
    let error_tx = tx.clone();
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            let _ = tx.send(convert(data, channels));
        },
        move |error| {
            let _ = error_tx.send(Event::Error(error));
        },
        None,
    )
}

fn audio_event<T>(data: &[T], channels: usize) -> Event
where
    T: Sample,
    i16: FromSample<T>,
{
    Event::Audio(downmix_i16(data, channels))
}

fn resample_event<T>(data: &[T], channels: usize) -> Event
where
    T: Sample,
    f32: FromSample<T>,
{
    Event::AudioToResample(downmix_f32(data, channels))
}

fn downmix_i16<T>(data: &[T], channels: usize) -> Vec<i16>
where
    T: Sample,
    i16: FromSample<T>,
{
    data.chunks_exact(channels)
        .map(|frame| {
            let sum = frame
                .iter()
                .map(|&sample| i64::from(sample.to_sample::<i16>()))
                .sum::<i64>();
            (sum / channels as i64) as i16
        })
        .collect()
}

fn downmix_f32<T>(data: &[T], channels: usize) -> Vec<f32>
where
    T: Sample,
    f32: FromSample<T>,
{
    let scale = 1.0 / channels as f32;
    data.chunks_exact(channels)
        .map(|frame| {
            frame
                .iter()
                .map(|&sample| sample.to_sample::<f32>())
                .sum::<f32>()
                * scale
        })
        .collect()
}

#[derive(Clone)]
pub struct Stopper {
    tx: Sender<Event>,
}

impl Stopper {
    pub fn stop(&self) {
        let _ = self.tx.send(Event::Stop);
    }
}

pub struct Recorder {
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Recorder {
    pub fn new() -> (Self, Stopper) {
        let (tx, rx) = channel();
        (Self { tx: tx.clone(), rx }, Stopper { tx })
    }

    pub fn record<F>(self, timeout: Duration, mut on_audio: F) -> anyhow::Result<StopReason>
    where
        F: FnMut(&[i16]) -> CallbackAction,
    {
        let (stream, input_rate) = make_stream(self.tx)?;
        let mut resampler = (input_rate != SAMPLE_RATE)
            .then(|| Resampler::new(input_rate))
            .transpose()?;

        stream.play().context("failed to start capture stream")?;
        eprintln!("Recording started");

        let deadline = Instant::now()
            .checked_add(timeout)
            .context("recording timeout is too large")?;

        let result = loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break Ok(StopReason::Timeout);
            };

            let audio = match self.rx.recv_timeout(remaining) {
                Ok(Event::Audio(audio)) => audio,
                Ok(Event::AudioToResample(audio)) => resampler
                    .as_mut()
                    .expect("non-16 kHz input has a resampler")
                    .push(&audio)?,
                Ok(Event::Stop) => break Ok(StopReason::Stopped),
                Ok(Event::Error(error)) => break Err(error),
                Err(RecvTimeoutError::Timeout) => break Ok(StopReason::Timeout),
                Err(RecvTimeoutError::Disconnected) => break Ok(StopReason::Stopped),
            };

            if !audio.is_empty() && on_audio(&audio) == CallbackAction::Stop {
                break Ok(StopReason::Stopped);
            }
        };

        drop(stream);

        Ok(result?)
    }
}
