//! Main Bark pipeline: push audio frames, finalise, get text.

use crate::config::BarkConfig;
use crate::error::{BarkError, Result};
use crate::ogg_opus::OpusOggEncoder;
use crate::postprocessor;
use crate::preprocessor;
use crate::whisper::WhisperClient;

/// The core pipeline object.
///
/// Callers create a `Bark` instance, push raw i16 audio frames into it, and
/// then call [`finalize`](Self::finalize) to run the full pipeline and get
/// the transcribed text.
///
/// ```no_run
/// use bark_core::{Bark, BarkConfig};
///
/// let config = BarkConfig::default();
/// let mut bark = Bark::new(config);
///
/// let frames: Vec<i16> = vec![0; 16000]; // 1 second of silence
/// bark.push_audio(&frames);
///
/// let text = bark.finalize().unwrap();
/// println!("Transcription: {text}");
/// ```
pub struct Bark {
    config: BarkConfig,
    audio: Vec<i16>,
}

impl Bark {
    /// Create a new Bark pipeline with the given configuration.
    pub fn new(config: BarkConfig) -> Self {
        Self {
            config,
            audio: Vec::new(),
        }
    }

    /// Feed raw PCM audio frames (i16, 16 kHz, mono).
    /// Can be called multiple times; frames are appended to an internal buffer.
    pub fn push_audio(&mut self, frames: &[i16]) {
        self.audio.extend_from_slice(frames);
    }

    /// Return the number of seconds of audio buffered so far.
    pub fn duration_secs(&self) -> f64 {
        self.audio.len() as f64 / 16_000.0
    }

    /// Consume the pipeline, process the buffered audio, and return the
    /// transcribed text.
    ///
    /// Steps:
    /// 1. Convert i16 → f32.
    /// 2. Run pre-processing (AGC, VAD).
    /// 3. Encode to OGG/Opus.
    /// 4. Transcribe via Whisper API.
    /// 5. Post-process the text.
    pub fn finalize(self) -> Result<String> {
        if self.audio.is_empty() {
            return Ok(String::new());
        }

        // 1. i16 → f32, normalised to [−1.0, 1.0]
        let mut audio_f32: Vec<f32> = self.audio.iter().map(|&s| s as f32 / 32768.0).collect();

        // 2. Pre-processing
        let processed = preprocessor::preprocess(&mut audio_f32, &self.config.pre)?;
        let audio_f32 = match processed {
            Some(a) => a,
            None => return Ok(String::new()), // no speech detected
        };

        if audio_f32.is_empty() {
            return Ok(String::new());
        }

        // 3. Encode to OGG/Opus
        let encoder = OpusOggEncoder::new(self.config.engine.bitrate_kbps)?;
        let ogg_data = encoder.encode_all(&audio_f32)?;

        if ogg_data.is_empty() {
            return Err(BarkError::Transcription("Opus encoder produced no output".into()));
        }

        // 4. Transcribe via HTTP
        let client = WhisperClient::new(self.config.engine);

        // We need a tokio runtime.  Create one if not already in an async context.
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| BarkError::Transcription(format!("Failed to create tokio runtime: {e}")))?;
        let text = rt.block_on(client.transcribe(&ogg_data))?;

        if text.is_empty() {
            return Ok(String::new());
        }

        // 5. Post-processing
        let final_text = postprocessor::postprocess(&text, &self.config.post);

        Ok(final_text)
    }
}