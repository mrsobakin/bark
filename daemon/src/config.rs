use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context};
use bark_core::BarkConfig;
use serde::Deserialize;

use crate::APP_NAME;

fn default_runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

#[derive(Clone)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub pipeline: BarkConfig,
}

#[derive(Clone)]
pub struct DaemonConfig {
    pub typer: Vec<String>,
    pub indicator_file: PathBuf,
    pub pidfile: PathBuf,
    pub timeout: Duration,
}

pub fn load(path: &Path) -> anyhow::Result<Config> {
    let raw = if path.exists() {
        warn_if_world_readable(path);
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config: {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse config: {}", path.display()))?
    } else {
        eprintln!("Config not found at {}; using defaults", path.display());
        RawConfig::default()
    };

    Config::try_from(raw)
}

impl TryFrom<RawConfig> for Config {
    type Error = anyhow::Error;

    fn try_from(raw: RawConfig) -> anyhow::Result<Self> {
        let daemon = DaemonConfig::try_from(raw.daemon)?;
        let pipeline = raw.pipeline;
        Ok(Self { daemon, pipeline })
    }
}

impl TryFrom<RawDaemonConfig> for DaemonConfig {
    type Error = anyhow::Error;

    fn try_from(raw: RawDaemonConfig) -> anyhow::Result<Self> {
        if raw.typer.is_empty() {
            bail!("daemon.typer must not be empty");
        }

        let runtime_dir = raw.runtime_dir.unwrap_or_else(default_runtime_dir);
        let indicator_file = raw
            .indicator_file
            .unwrap_or_else(|| runtime_dir.join(format!("{APP_NAME}.state")));
        let pidfile = raw
            .pidfile
            .unwrap_or_else(|| runtime_dir.join(format!("{APP_NAME}.pid")));
        let timeout = Duration::from_secs_f64(raw.timeout.unwrap_or(raw.recorder.timeout).max(0.1));

        Ok(Self {
            typer: raw.typer,
            indicator_file,
            pidfile,
            timeout,
        })
    }
}

pub fn validate_pipeline(pipeline: &BarkConfig) -> anyhow::Result<()> {
    if pipeline.engine.api_key.is_empty() && pipeline.engine.endpoint.contains("groq.com") {
        bail!("Groq API key missing; set pipeline.engine.api_key");
    }

    Ok(())
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct RawConfig {
    daemon: RawDaemonConfig,
    pipeline: BarkConfig,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct RawDaemonConfig {
    typer: Vec<String>,
    runtime_dir: Option<PathBuf>,
    indicator_file: Option<PathBuf>,
    pidfile: Option<PathBuf>,
    timeout: Option<f64>,
    recorder: RawRecorderConfig,
}

#[derive(Clone, Deserialize)]
#[serde(default)]
struct RawRecorderConfig {
    timeout: f64,
}

impl Default for RawRecorderConfig {
    fn default() -> Self {
        Self { timeout: 300.0 }
    }
}

#[cfg(unix)]
fn warn_if_world_readable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    if let Ok(metadata) = path.metadata() {
        if metadata.permissions().mode() & 0o044 != 0 {
            eprintln!(
                "Warning: {} is group/world readable; consider chmod 600",
                path.display()
            );
        }
    }
}

#[cfg(not(unix))]
fn warn_if_world_readable(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_defaults_and_parses_native_config() {
        let raw: RawConfig = toml::from_str(
            r#"
[daemon]
typer = ["cat"]

[pipeline.engine]
api_key = "key"

[pipeline.pre.agc]
target_db = -20.0

[pipeline.pre.vad]
threshold = 0.5
"#,
        )
        .unwrap();

        let cfg = Config::try_from(raw).unwrap();
        assert!(cfg.pipeline.pre.agc.is_some());
        assert!(cfg.pipeline.pre.vad.is_some());
        assert!(cfg.daemon.pidfile.ends_with(format!("{APP_NAME}.pid")));
    }
}
