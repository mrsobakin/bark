mod config;
mod daemon;
mod indicator;
mod pidfile;
mod recorder;
mod typer;

use std::path::PathBuf;

use anyhow::bail;

const APP_NAME: &str = "barkd";

fn default_config_path() -> PathBuf {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    config_home.join(APP_NAME).join("config.toml")
}

fn default_pidfile_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(format!("{APP_NAME}.pid"))
}

fn main() -> anyhow::Result<()> {
    match parse_args()? {
        Command::Run { config_path } => {
            let config = config::load(&config_path)?;
            daemon::run(config)
        }
        Command::Toggle { pidfile } => daemon::toggle(&pidfile),
        Command::Help => {
            print_help();
            Ok(())
        }
    }
}

#[derive(Debug)]
enum Command {
    Run { config_path: PathBuf },
    Toggle { pidfile: PathBuf },
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    Toggle,
    Help,
}

fn parse_args() -> anyhow::Result<Command> {
    let mut args = std::env::args_os().skip(1);
    let mut mode = Mode::Run;
    let mut config_path = None;
    let mut pidfile = None;

    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "-h" | "--help" => mode = Mode::Help,
            "--toggle" => mode = Mode::Toggle,
            "--config" => {
                let Some(path) = args.next() else {
                    bail!("--config requires a path");
                };
                config_path = Some(path.into());
            }
            "--pidfile" => {
                let Some(path) = args.next() else {
                    bail!("--pidfile requires a path");
                };
                pidfile = Some(path.into());
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    match mode {
        Mode::Run => Ok(Command::Run {
            config_path: config_path.unwrap_or_else(default_config_path),
        }),
        Mode::Toggle => Ok(Command::Toggle {
            pidfile: pidfile.unwrap_or_else(default_pidfile_path),
        }),
        Mode::Help => Ok(Command::Help),
    }
}

fn print_help() {
    println!(
        "{APP_NAME} - speech-to-text daemon\n\n\
Usage:\n\
  {APP_NAME} [--config PATH]          Run daemon\n\
  {APP_NAME} --toggle [--pidfile P]   Start/stop recording\n\n\
The daemon waits for SIGUSR1 when idle. `--toggle` sends SIGUSR1:\n\
first toggle starts recording; second toggle stops recording."
    );
}
