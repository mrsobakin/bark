use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, Stream, StreamError};

use bark_core::SAMPLE_RATE;

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

fn make_stream<D, E>(mut on_audio: D, on_error: E) -> anyhow::Result<Stream>
where
    D: FnMut(&[i16]) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no microphone found; check your audio input devices")?;

    eprintln!("Device: {}", device.name()?);

    let config = device
        .supported_input_configs()?
        .filter_map(|c| c.try_with_sample_rate(SampleRate(SAMPLE_RATE)))
        .find(|c| c.sample_format() == SampleFormat::I16 && c.channels() == 1)
        .ok_or_else(|| anyhow!("input device does not support i16 @ 16000 Hz"))?
        .config();

    Ok(device.build_input_stream(&config, move |data, _| on_audio(data), on_error, None)?)
}

enum Event {
    Stop,
    Error(StreamError),
    Data(Vec<i16>),
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
        let (tx1, tx2) = (self.tx.clone(), self.tx);

        let stream = make_stream(
            move |d| {
                let _ = tx1.send(Event::Data(d.to_vec()));
            },
            move |e| {
                let _ = tx2.send(Event::Error(e));
            },
        )?;

        stream.play().context("failed to start capture stream")?;
        eprintln!("Recording started");

        let deadline = Instant::now()
            .checked_add(timeout)
            .context("recording timeout is too large")?;

        let result = loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break Ok(StopReason::Timeout);
            };

            match self.rx.recv_timeout(remaining) {
                Ok(Event::Data(buf)) => {
                    if on_audio(&buf) == CallbackAction::Stop {
                        break Ok(StopReason::Stopped);
                    }
                }
                Ok(Event::Stop) => break Ok(StopReason::Stopped),
                Ok(Event::Error(e)) => break Err(e),
                Err(RecvTimeoutError::Timeout) => break Ok(StopReason::Timeout),
                Err(RecvTimeoutError::Disconnected) => break Ok(StopReason::Stopped),
            }
        };

        drop(stream);

        Ok(result?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "manual"]
    fn test_record() {
        let mut audio = vec![];

        let (rec, _) = Recorder::new();

        let res = rec.record(Duration::from_secs(5), |data| {
            audio.extend_from_slice(data);
            CallbackAction::Continue
        });

        let Ok(StopReason::Timeout) = res else {
            panic!("wrong stop reason");
        };

        let data = audio
            .into_iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<u8>>();

        std::fs::create_dir_all("test_output").unwrap();
        std::fs::write("test_output/audio.pcm", &data).unwrap();
    }
}
