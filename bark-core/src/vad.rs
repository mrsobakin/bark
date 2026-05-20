//! Voice Activity Detection.
//!
//! The core [`VoiceDetector`] trait lets callers plug in any speech / non-speech
//! classifier.  A Silero ONNX implementation is available behind the
//! `vad-silero` feature flag.
//!
//! [`VadProcessor`] wraps a detector with the same state-machine logic used in
//! the Android and Python clients: an attack buffer recovers audio just
//! before speech is confirmed, silence gaps up to `max_silence_ms` are
//! preserved between segments, and longer gaps are dropped.

use crate::config::VadConfig;

/// Sample rate used throughout the pipeline.
const SAMPLE_RATE: u32 = 16_000;

/// Frame size in samples that the Silero model expects.
pub const VAD_FRAME_SAMPLES: usize = 512;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// A binary speech / non-speech classifier operating on a single audio frame.
pub trait VoiceDetector: Send {
    /// Returns `true` when `frame` (16 kHz mono f32, `VAD_FRAME_SAMPLES`
    /// samples long) contains speech.
    fn is_speech(&mut self, frame: &[f32]) -> bool;

    /// Reset internal state so the detector can be reused on a new recording.
    fn reset(&mut self);
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// Frame-by-frame VAD processor implementing the same logic as the Android
/// `VADProcessor` and the Python `VoiceActivityDetector`:
///
/// - A rolling **attack buffer** recovers audio preceding confirmed speech.
/// - A minimum-speech-frames threshold prevents noise spikes from opening.
/// - A minimum-silence-frames threshold keeps small pauses inside a segment.
/// - Silence gaps between segments are capped at `max_silence_ms`.
pub struct VadProcessor<D: VoiceDetector> {
    detector: D,
    in_speech: bool,
    consecutive_speech: u32,
    consecutive_silence: u32,
    silence_written: u32,
    attack_buffer: Vec<Vec<f32>>,

    // Config (derived from VadConfig, in frames)
    min_speech_frames: u32,
    min_silence_frames: u32,
    max_silence_frames: u32,
    attack_frames: u32,
}

impl<D: VoiceDetector> VadProcessor<D> {
    pub fn new(detector: D, config: &VadConfig) -> Self {
        let ms_to_frames = |ms: u32| -> u32 { ms * SAMPLE_RATE / (VAD_FRAME_SAMPLES as u32 * 1000) };

        Self {
            detector,
            in_speech: false,
            consecutive_speech: 0,
            consecutive_silence: 0,
            silence_written: 0,
            attack_buffer: Vec::new(),
            min_speech_frames: ms_to_frames(config.min_speech_ms),
            min_silence_frames: ms_to_frames(config.min_silence_ms),
            max_silence_frames: ms_to_frames(config.max_silence_ms),
            attack_frames: ms_to_frames(config.attack_ms),
        }
    }

    /// Feed a single VAD-sized frame (512 f32 samples) and return audio
    /// samples that should be kept (speech + short silence gaps).
    pub fn process(&mut self, frame: &[f32]) -> Vec<f32> {
        let is_speech = self.detector.is_speech(frame);
        let mut out = Vec::new();

        if is_speech {
            self.on_speech_frame(frame, &mut out);
        } else {
            self.on_silence_frame(frame, &mut out);
        }

        out
    }

    /// Process an entire buffer of f32 audio.  Returns the VAD-filtered audio.
    pub fn process_buffer(&mut self, audio: &[f32]) -> Vec<f32> {
        let mut result = Vec::new();
        let frame_size = VAD_FRAME_SAMPLES;

        for chunk in audio.chunks(frame_size) {
            if chunk.len() < frame_size {
                // Partial final frame – pass through if currently in speech.
                if self.in_speech {
                    result.extend_from_slice(chunk);
                }
                continue;
            }

            let out = self.process(chunk);
            result.extend_from_slice(&out);
        }

        result
    }

    /// Reset internal state so the processor can be reused.
    pub fn reset(&mut self) {
        self.detector.reset();
        self.in_speech = false;
        self.consecutive_speech = 0;
        self.consecutive_silence = 0;
        self.silence_written = 0;
        self.attack_buffer.clear();
    }

    // -- private helpers --

    fn on_speech_frame(&mut self, frame: &[f32], out: &mut Vec<f32>) {
        self.consecutive_speech += 1;
        self.consecutive_silence = 0;

        if self.in_speech {
            out.extend_from_slice(frame);
        } else {
            // Buffer in the attack window.
            self.attack_buffer.push(frame.to_vec());
            if self.attack_buffer.len() > self.attack_frames as usize {
                self.attack_buffer.remove(0);
            }

            if self.consecutive_speech >= self.min_speech_frames {
                self.in_speech = true;
                self.silence_written = 0;
                for buf in &self.attack_buffer {
                    out.extend_from_slice(buf);
                }
                self.attack_buffer.clear();
            }
        }
    }

    fn on_silence_frame(&mut self, frame: &[f32], out: &mut Vec<f32>) {
        self.consecutive_silence += 1;
        self.consecutive_speech = 0;

        if self.in_speech {
            out.extend_from_slice(frame);
            if self.consecutive_silence >= self.min_silence_frames {
                self.in_speech = false;
                self.attack_buffer.clear();
            }
        } else {
            // Rolling attack buffer (silence frames also go in, in case
            // speech starts later).
            self.attack_buffer.push(frame.to_vec());
            if self.attack_buffer.len() > self.attack_frames as usize {
                self.attack_buffer.remove(0);
            }

            if self.silence_written < self.max_silence_frames {
                let mut zeros = vec![0.0f32; frame.len()];
                out.append(&mut zeros);
                self.silence_written += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Silero ONNX backend (behind feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "vad-silero")]
mod silero {
    use super::{VoiceDetector, VAD_FRAME_SAMPLES};
    use crate::error::BarkError;
    use std::path::Path;

    /// Silero VAD model loaded from an ONNX file.
    pub struct SileroVad {
        session: ort::session::Session,
        threshold: f32,
        h: Vec<f32>,
        c: Vec<f32>,
    }

    impl SileroVad {
        /// Load the Silero VAD model from `path` (ONNX format).
        pub fn from_file(path: &Path) -> Result<Self, BarkError> {
            let mut session = ort::session::Session::builder()
                .map_err(|e| BarkError::OnnxRuntime(e.to_string()))?;
        let session = session.commit_from_file(path)
                .map_err(|e| BarkError::OnnxRuntime(e.to_string()))?;

            Ok(Self {
                session,
                threshold: 0.5,
                h: vec![0.0; 128], // [2, 1, 64] flattened
                c: vec![0.0; 128],
            })
        }

        /// Create with a custom speech probability threshold.
        pub fn with_threshold(mut self, threshold: f32) -> Self {
            self.threshold = threshold;
            self
        }
    }

    impl VoiceDetector for SileroVad {
        fn is_speech(&mut self, frame: &[f32]) -> bool {
            debug_assert_eq!(frame.len(), VAD_FRAME_SAMPLES);

            use ort::value::Tensor;

            let input_val = Tensor::from_array((
                vec![1i64, VAD_FRAME_SAMPLES as i64],
                frame.to_vec().into_boxed_slice(),
            )).unwrap();

            let sr_val = Tensor::from_array((vec![1i64], vec![16000i64].into_boxed_slice())).unwrap();
            let h_val = Tensor::from_array((
                vec![2i64, 1i64, 64i64],
                self.h.clone().into_boxed_slice(),
            )).unwrap();
            let c_val = Tensor::from_array((
                vec![2i64, 1i64, 64i64],
                self.c.clone().into_boxed_slice(),
            )).unwrap();

            let result = self.session.run(ort::inputs![
                input_val,
                sr_val,
                h_val,
                c_val
            ]);

            match result {
                Ok(outputs) => {
                    // Output 0: speech probability [1, 1]
                    let prob = outputs[0]
                        .try_extract_tensor::<f32>()
                        .ok()
                        .and_then(|(_, data)| data.first().copied())
                        .unwrap_or(0.0);

                    // Extract updated hidden/cell states (outputs 1, 2).
                    if outputs.len() > 1 {
                        if let Ok((_, hn)) = outputs[1].try_extract_tensor::<f32>() {
                            self.h = hn.to_vec();
                        }
                    }
                    if outputs.len() > 2 {
                        if let Ok((_, cn)) = outputs[2].try_extract_tensor::<f32>() {
                            self.c = cn.to_vec();
                        }
                    }

                    prob > self.threshold
                }
                Err(_) => false,
            }
        }

        fn reset(&mut self) {
            self.h = vec![0.0; 128];
            self.c = vec![0.0; 128];
        }
    }
}

#[cfg(feature = "vad-silero")]
pub use silero::SileroVad;

// ---------------------------------------------------------------------------
// Simple energy-based fallback detector (always available)
// ---------------------------------------------------------------------------

/// A trivial energy-based "voice detector" that classifies a frame as speech
/// whenever its RMS energy exceeds `threshold`.
///
/// This is a **rough** fallback when Silero is not available.
pub struct EnergyVad {
    /// RMS threshold in [0.0, 1.0].  `0.02` is a reasonable starting point.
    pub threshold: f32,
}

impl EnergyVad {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl VoiceDetector for EnergyVad {
    fn is_speech(&mut self, frame: &[f32]) -> bool {
        if frame.is_empty() {
            return false;
        }
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        rms > self.threshold
    }

    fn reset(&mut self) {}
}