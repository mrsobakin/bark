mod audio;
mod config;
mod daemon;
mod indicator;
mod pidfile;
mod transcribe;
mod typer;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use transcribe::TranscribeArgs;

const APP_NAME: &str = "barkd";

fn default_config_path() -> PathBuf {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    config_home.join(APP_NAME).join("config.toml")
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(default_config_path);
    let config = config::load(&config_path)?;

    match cli.command.unwrap_or(Command::Daemon) {
        Command::Daemon => daemon::run(config),
        Command::Toggle => daemon::toggle(&config.daemon.pidfile),
        Command::Transcribe(args) => transcribe::run(config, args),
    }
}

#[derive(Debug, Parser)]
#[command(name = APP_NAME, version, about = "speech-to-text daemon")]
struct Cli {
    /// Config file path.
    #[arg(short, long)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run background daemon.
    Daemon,

    /// Toggle daemon recording.
    Toggle,

    /// Record once and output transcription or preview.
    Transcribe(TranscribeArgs),
}
