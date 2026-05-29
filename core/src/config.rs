use serde::Deserialize;

/// Top-level configuration for the Bark pipeline.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BarkConfig {
    /// Pre-processing options (applied before encoding).
    pub pre: PreConfig,

    /// Post-processing options (applied to the transcription text).
    pub post: PostConfig,

    /// Whisper / transcription engine.
    pub engine: EngineConfig,
}

/// Pre-processing configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PreConfig {
    /// Enable automatic gain control (loudness normalization).
    pub agc: Option<AgcConfig>,

    /// Enable voice-activity detection.
    pub vad: Option<VadConfig>,
}

/// AGC (automatic gain control) configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgcConfig {
    /// Target speech loudness in dBFS.
    pub target_db: f32,

    /// Maximum gain boost allowed in dB.
    pub max_gain_db: f32,

    /// Gain attack time in ms.
    pub attack_ms: f32,

    /// Gain release time in ms.
    pub release_ms: f32,

    /// RMS smoothing window in ms.
    pub rms_window_ms: f32,

    /// Long-term RMS window in ms.
    pub long_window_ms: f32,

    /// High-pass filter cutoff in Hz.
    pub high_pass_hz: f32,
}

impl Default for AgcConfig {
    fn default() -> Self {
        Self {
            target_db: -18.0,
            max_gain_db: 20.0,
            attack_ms: 30.0,
            release_ms: 500.0,
            rms_window_ms: 80.0,
            long_window_ms: 1500.0,
            high_pass_hz: 80.0,
        }
    }
}

/// VAD (voice-activity detection) configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct VadConfig {
    /// Speech probability threshold (0.0–1.0).
    pub threshold: f32,

    /// Minimum speech segment duration in ms.
    pub min_speech_ms: u32,

    /// Minimum silence duration to end a speech segment, in ms.
    pub min_silence_ms: u32,

    /// Maximum silence gap preserved between speech segments, in ms.
    pub max_silence_ms: u32,

    /// Pre-speech attack buffer length in ms.
    /// Audio in the rolling buffer is recovered when speech is confirmed.
    pub attack_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_speech_ms: 100,
            min_silence_ms: 150,
            max_silence_ms: 500,
            attack_ms: 150,
        }
    }
}

/// Post-processing configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PostConfig {}

/// Whisper-compatible transcription engine configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    /// Full URL of the transcription endpoint,
    /// e.g. `https://api.groq.com/openai/v1/audio/transcriptions`.
    #[serde(default = "default_endpoint")]
    pub endpoint: String,

    /// API key (sent as `Authorization: Bearer <key>`).
    #[serde(default)]
    pub api_key: String,

    /// Whisper model name.
    #[serde(default = "default_model")]
    pub model: String,

    /// Language code (omit for auto-detect).
    #[serde(default)]
    pub language: Option<String>,

    /// Initial prompt for the model.
    #[serde(default)]
    pub prompt: Option<String>,
}

fn default_model() -> String {
    "whisper-large-v3-turbo".into()
}

fn default_endpoint() -> String {
    "https://api.groq.com/openai/v1/audio/transcriptions".into()
}

impl Default for BarkConfig {
    fn default() -> Self {
        Self {
            pre: PreConfig::default(),
            post: PostConfig::default(),
            engine: EngineConfig {
                endpoint: default_endpoint(),
                api_key: String::new(),
                model: default_model(),
                language: None,
                prompt: None,
            },
        }
    }
}
