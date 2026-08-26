use anyhow::{Context, Result};
use std::thread;
use std::time::Duration;

/// Paste text by writing to clipboard and simulating Cmd+V (macOS).
pub fn paste_text(text: &str, delay_ms: u64, restore: bool) -> Result<()> {
    let saved_clipboard = if restore { get_clipboard().ok() } else { None };

    set_clipboard(text)?;
    thread::sleep(Duration::from_millis(delay_ms));

    synth_cmd_v()?;
    thread::sleep(Duration::from_millis(120));

    if let Some(saved) = saved_clipboard {
        set_clipboard(&saved)?;
    }

    Ok(())
}

pub fn copy_to_clipboard(text: &str) -> Result<()> {
    set_clipboard(text)
}

#[cfg(target_os = "macos")]
fn set_clipboard(text: &str) -> Result<()> {
    use std::process::Command;
    Command::new("pbcopy")
        .arg(text)
        .output()
        .context("Failed to run pbcopy")?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn get_clipboard() -> Result<String> {
    use std::process::Command;
    let output = Command::new("pbpaste")
        .output()
        .context("Failed to run pbpaste")?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(target_os = "macos")]
fn synth_cmd_v() -> Result<()> {
    use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventFlags};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("Failed to create event source"))?;

    // Cmd down (virtual key 0x09 = V, but we use set_string for paste)
    let cmd_down = CGEvent::new_keyboard_event(source.clone(), 0x09, true)
        .map_err(|_| anyhow::anyhow!("Failed to create Cmd down"))?;
    cmd_down.set_flags(CGEventFlags::CGEventFlagCommand);
    cmd_down.post(CGEventTapLocation::HID);
    thread::sleep(Duration::from_millis(2));

    // V down
    let v_down = CGEvent::new_keyboard_event(source.clone(), 0x09, true)
        .map_err(|_| anyhow::anyhow!("Failed to create V down"))?;
    v_down.set_flags(CGEventFlags::CGEventFlagCommand);
    v_down.post(CGEventTapLocation::HID);
    thread::sleep(Duration::from_millis(2));

    // V up
    let v_up = CGEvent::new_keyboard_event(source.clone(), 0x09, false)
        .map_err(|_| anyhow::anyhow!("Failed to create V up"))?;
    v_up.set_flags(CGEventFlags::CGEventFlagCommand);
    v_up.post(CGEventTapLocation::HID);
    thread::sleep(Duration::from_millis(2));

    // Cmd up
    let cmd_up = CGEvent::new_keyboard_event(source.clone(), 0x09, false)
        .map_err(|_| anyhow::anyhow!("Failed to create Cmd up"))?;
    cmd_up.post(CGEventTapLocation::HID);

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn set_clipboard(text: &str) -> Result<()> {
    use std::process::Command;
    Command::new("xclip")
        .args(["-selection", "clipboard"])
        .arg(text)
        .output()
        .or_else(|_| {
            Command::new("xsel")
                .args(["--clipboard", "--input"])
                .arg(text)
                .output()
        })
        .context("No clipboard tool found")?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn get_clipboard() -> Result<String> {
    use std::process::Command;
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
        .context("Failed to read clipboard")?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(not(target_os = "macos"))]
fn synth_cmd_v() -> Result<()> {
    use std::process::Command;
    Command::new("xdotool")
        .args(["key", "ctrl+v"])
        .output()
        .context("Failed to simulate Ctrl+V")?;
    Ok(())
}
