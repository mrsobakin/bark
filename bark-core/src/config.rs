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
    /// Target loudness in dB.  -23 is the broadcast standard (EBU R128).
    pub target_db: f32,
}

impl Default for AgcConfig {
    fn default() -> Self {
        Self { target_db: -23.0 }
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
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PostConfig {
}

/// Whisper-compatible transcription engine configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    /// Full URL of the transcription endpoint,
    /// e.g. `https://api.groq.com/openai/v1/audio/transcriptions`.
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

impl Default for BarkConfig {
    fn default() -> Self {
        Self {
            pre: PreConfig::default(),
            post: PostConfig::default(),
            engine: EngineConfig {
                endpoint: "https://api.groq.com/openai/v1/audio/transcriptions".into(),
                api_key: String::new(),
                model: default_model(),
                language: None,
                prompt: None,
            },
        }
    }
}

impl Default for PostConfig {
    fn default() -> Self {
        Self {
        }
    }
}
