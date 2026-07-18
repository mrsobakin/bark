use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Context};

pub fn type_text(command: &[String], text: &str) -> anyhow::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    if command.is_empty() {
        #[cfg(windows)]
        return type_text_windows(text);

        #[cfg(not(windows))]
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

#[cfg(windows)]
fn type_text_windows(text: &str) -> anyhow::Result<()> {
    use std::mem::size_of;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};

    let inputs = unicode_inputs(text);
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        let error = std::io::Error::last_os_error();
        bail!(
            "Windows injected {sent} of {} keyboard events: {error}",
            inputs.len()
        );
    }

    Ok(())
}

#[cfg(windows)]
fn unicode_inputs(text: &str) -> Vec<windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    };

    text.encode_utf16()
        .flat_map(|unit| {
            [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP].map(|flags| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            })
        })
        .collect()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT_KEYBOARD, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    };

    #[test]
    fn unicode_input_has_key_down_and_key_up_for_each_utf16_unit() {
        let inputs = unicode_inputs("A🐕");

        assert_eq!(inputs.len(), 6);
        assert!(inputs.iter().all(|input| input.r#type == INPUT_KEYBOARD));
        for pair in inputs.chunks_exact(2) {
            let down = unsafe { pair[0].Anonymous.ki };
            let up = unsafe { pair[1].Anonymous.ki };
            assert_eq!(down.wScan, up.wScan);
            assert_eq!(down.dwFlags, KEYEVENTF_UNICODE);
            assert_eq!(up.dwFlags, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP);
        }
    }
}
