use anyhow::Result;
use std::thread;
use std::time::Duration;

/// Type text character-by-character via CGEvent (macOS).
/// Safe for TUIs that use raw mode (Claude Code, Ink, etc.).
#[cfg(target_os = "macos")]
pub fn type_text(text: &str, delay_ms: u64) -> Result<()> {
    
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("Failed to create event source"))?;

    let delay = Duration::from_millis(delay_ms);

    for ch in text.chars() {
        if ch == '\n' {
            // Return key = keycode 36
            send_key(&source, 36, true)?;
            thread::sleep(delay);
            send_key(&source, 36, false)?;
            thread::sleep(delay);
            continue;
        }

        if ch == '\r' {
            continue;
        }

        if ch == '\t' {
            // Tab = keycode 48
            send_key(&source, 48, true)?;
            thread::sleep(delay);
            send_key(&source, 48, false)?;
            thread::sleep(delay);
            continue;
        }

        // Use CGEvent's set_string for Unicode characters
        type_char_event(&source, ch, delay)?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn send_key(
    source: &core_graphics::event_source::CGEventSource,
    keycode: u16,
    key_down: bool,
) -> Result<()> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};

    let event = CGEvent::new_keyboard_event(source.clone(), keycode, key_down)
        .map_err(|_| anyhow::anyhow!("Failed to create keyboard event"))?;
    event.set_flags(CGEventFlags::CGEventFlagNull);
    event.post(CGEventTapLocation::HID);
    Ok(())
}

#[cfg(target_os = "macos")]
fn type_char_event(
    source: &core_graphics::event_source::CGEventSource,
    ch: char,
    delay: Duration,
) -> Result<()> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};

    let utf16: Vec<u16> = ch.encode_utf16(&mut [0u16; 2]).to_vec();

    let event = CGEvent::new_keyboard_event(source.clone(), 0, true)
        .map_err(|_| anyhow::anyhow!("Failed to create unicode event"))?;
    event.set_flags(CGEventFlags::CGEventFlagNull);
    event.set_string_from_utf16_unchecked(&utf16);
    event.post(CGEventTapLocation::HID);
    thread::sleep(delay);

    let event_up = CGEvent::new_keyboard_event(source.clone(), 0, false)
        .map_err(|_| anyhow::anyhow!("Failed to create unicode event up"))?;
    event_up.set_flags(CGEventFlags::CGEventFlagNull);
    event_up.set_string_from_utf16_unchecked(&utf16);
    event_up.post(CGEventTapLocation::HID);
    thread::sleep(delay);

    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn type_text(text: &str, delay_ms: u64) -> Result<()> {
    use std::process::Command;
    Command::new("xdotool")
        .args(["type", "--delay", &delay_ms.to_string(), text])
        .output()
        .context("Failed to run xdotool type")?;
    Ok(())
}
