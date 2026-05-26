use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Context};

pub fn type_text(command: &[String], text: &str) -> anyhow::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    if command.is_empty() {
        bail!("typer command is empty");
    }

    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start typer: {}", command[0]))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        bail!("typer exited with status {status}");
    }

    Ok(())
}
