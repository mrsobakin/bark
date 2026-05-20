//! Pre-processing: automatic gain control and voice-activity detection.

use crate::config::{AgcConfig, PreConfig, VadConfig};
use crate::error::{BarkError, Result};
use crate::vad::{EnergyVad, VadProcessor};

// Sample rate used throughout the pipeline.
#[allow(dead_code)]
const SAMPLE_RATE: u32 = 16_000;

// ---------------------------------------------------------------------------
// AGC
// ---------------------------------------------------------------------------

/// Automatic gain control – loudness normalisation.
///
/// Computes the RMS loudness of the signal and applies a gain factor to
/// reach `target_db`.  This is a simplified version of EBU R128; a full
/// K-weighted LUFS implementation can be swapped in later.
pub struct Agc {
    target_db: f32,
}

impl Agc {
    pub fn new(config: &AgcConfig) -> Self {
        Self {
            target_db: config.target_db,
        }
    }

    /// Return the default AGC (−23 dBFS target).
    #[allow(dead_code)]
    pub fn default() -> Self {
        Self {
            target_db: -23.0,
        }
    }

    /// Normalise `audio` in-place (f32, mono, 16 kHz) to the target loudness.
    /// If the signal is too short or silent, it is returned unchanged.
    pub fn process(&self, audio: &mut [f32]) {
        const MIN_SAMPLES: usize = 400; // ~25 ms at 16 kHz
        if audio.len() < MIN_SAMPLES {
            return;
        }

        // Compute RMS.
        let sum_sq: f32 = audio.iter().map(|s| s * s).sum();
        let mean_sq = sum_sq / audio.len() as f32;
        if mean_sq <= 0.0 {
            return;
        }
        let rms_db = 10.0 * mean_sq.log10();

        let gain_db = self.target_db - rms_db;
        let gain = 10.0_f32.powf(gain_db / 20.0);

        for s in audio.iter_mut() {
            *s = (*s * gain).clamp(-1.0, 1.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Full pre-processing pipeline
// ---------------------------------------------------------------------------

/// Applies the enabled pre-processing steps to a buffer of f32 audio samples
/// (16 kHz mono, nominal range −1.0 … 1.0).
///
/// Returns the processed audio.  If VAD determines that no speech is present,
/// returns `Ok(None)`.
pub fn preprocess(audio: &mut Vec<f32>, config: &PreConfig) -> Result<Option<Vec<f32>>> {
    // 1. AGC
    if let Some(ref agc_cfg) = config.agc {
        let agc = Agc::new(agc_cfg);
        agc.process(audio);
    }

    // 2. VAD
    if let Some(ref vad_cfg) = config.vad {
        let processed = run_vad(audio, vad_cfg)?;
        if processed.is_empty() {
            return Ok(None);
        }
        *audio = processed;
    }

    Ok(Some(audio.clone()))
}

/// Run the VAD pipeline on `audio` and return the filtered result.
fn run_vad(audio: &[f32], config: &VadConfig) -> Result<Vec<f32>> {
    #[cfg(feature = "vad-silero")]
    {
        if !config.model_path.is_empty() {
            let detector = crate::vad::SileroVad::from_file(std::path::Path::new(&config.model_path))
                .map_err(|e| BarkError::Vad(format!("Failed to load Silero model: {e}")))?;
            let mut processor = VadProcessor::new(detector, config);
            return Ok(processor.process_buffer(audio));
        }
    }

    // Fallback to energy-based VAD when Silero is unavailable or no model
    // path is configured.
    let detector = EnergyVad::new(0.02);
    let mut processor = VadProcessor::new(detector, config);
    Ok(processor.process_buffer(audio))
}