use anyhow::{Context, Result};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Pressed,
    Released,
}

pub struct HotkeyManager {
    rx: Receiver<HotkeyAction>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    combo: String,
}

impl HotkeyManager {
    /// Sets up a global hotkey. `modifier` is a `+`-separated list of
    /// Command / Option / Control / Shift. If `key` is empty the hotkey is
    /// modifier-only: it fires on Pressed/Released of the modifier combo,
    /// which is ideal for hold-to-talk.
    pub fn new(key: &str, modifier: &str, running: Arc<AtomicBool>) -> Result<Self> {
        let mods = parse_modifiers(modifier);
        let keycode = parse_key(key)?;
        let (tx, rx) = mpsc::channel::<HotkeyAction>();

        let combo = if key.is_empty() {
            modifier.to_string()
        } else {
            format!("{}+{}", modifier, key)
        };

        let handle = spawn_tap(mods, keycode, rx_running_clone(&running), tx)
            .context("Failed to start hotkey event tap")?;

        tracing::info!("Hotkey listening: {} ({})", combo, describe(mods, keycode));

        Ok(HotkeyManager {
            rx,
            running,
            handle: Some(handle),
            combo,
        })
    }

    /// Blocks until the next hotkey event, returning the action.
    pub fn wait_for_action(&self) -> Option<HotkeyAction> {
        match self.rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(action) => Some(action),
            Err(mpsc::RecvTimeoutError::Disconnected) => None,
            Err(mpsc::RecvTimeoutError::Timeout) => Some(HotkeyAction::Released),
        }
    }

    pub fn combo(&self) -> &str {
        &self.combo
    }

    /// Signals the hotkey thread to stop listening.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn rx_running_clone(running: &Arc<AtomicBool>) -> Arc<AtomicBool> {
    Arc::clone(running)
}

fn spawn_tap(
    mods: CGEventFlags,
    keycode: Option<u16>,
    running: Arc<AtomicBool>,
    tx: Sender<HotkeyAction>,
) -> Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("miccli-hotkey".into())
        .spawn(move || {
            use core_foundation::runloop::CFRunLoop;

            let pressed = Arc::new(AtomicBool::new(false));

            let events = vec![
                CGEventType::FlagsChanged,
                CGEventType::KeyDown,
                CGEventType::KeyUp,
            ];

            let _tap = CGEventTap::with_enabled(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                events,
                move |_proxy, etype, event| {
                    // If the OS disables our tap, it means Accessibility permission
                    // is missing or was revoked — surface a useful message.
                    match etype {
                        CGEventType::TapDisabledByTimeout
                        | CGEventType::TapDisabledByUserInput => {
                            tracing::warn!(
                                "Event tap disabled by system — grant miccli Accessibility \
                                 permission (System Settings > Privacy & Security > Accessibility)"
                            );
                            return CallbackResult::Keep;
                        }
                        _ => {}
                    }

                    let was = pressed.load(Ordering::SeqCst);
                    let now = is_active(etype, event, mods, keycode);

                    match (was, now) {
                        (false, true) => {
                            pressed.store(true, Ordering::SeqCst);
                            tracing::info!("hotkey: pressed");
                            let _ = tx.send(HotkeyAction::Pressed);
                        }
                        (true, false) => {
                            pressed.store(false, Ordering::SeqCst);
                            tracing::info!("hotkey: released");
                            let _ = tx.send(HotkeyAction::Released);
                        }
                        _ => {}
                    }

                    // Keep listening (never swallow events).
                    CallbackResult::Keep
                },
                || {
                    // Run this run loop until the daemon asks us to stop.
                    let run_loop = CFRunLoop::get_current();
                    while running.load(Ordering::SeqCst) {
                        CFRunLoop::run_in_mode(
                            // SAFETY: immutable use of the process-wide default-mode const string.
                            unsafe { core_foundation::runloop::kCFRunLoopDefaultMode },
                            std::time::Duration::from_millis(25),
                            false,
                        );
                    }
                    run_loop.stop();
                },
            );

            if _tap.is_err() {
                tracing::error!("Failed to install event tap (check Accessibility permission)");
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to spawn hotkey thread: {e}"))
}

/// Returns whether the event indicates the target hotkey combo is currently
/// held (active).
fn is_active(
    etype: CGEventType,
    event: &CGEvent,
    mods: CGEventFlags,
    keycode: Option<u16>,
) -> bool {
    let flags = event.get_flags();

    match keycode {
        None => {
            // Modifier-only hold: active when all configured modifiers are set.
            if matches!(etype, CGEventType::FlagsChanged)
                || is_any_key_event(etype)
            {
                let all = if mods.is_empty() { false } else { flags.contains(mods) };
                all
            } else {
                false
            }
        }
        Some(kc) => {
            if is_any_key_event(etype) {
                let key_ok = event.get_integer_value_field(9) as u16 == kc;
                let mods_ok = if mods.is_empty() { true } else { flags.contains(mods) };
                key_ok && mods_ok
            } else {
                false
            }
        }
    }
}

fn is_any_key_event(etype: CGEventType) -> bool {
    matches!(etype, CGEventType::KeyDown | CGEventType::KeyUp)
}

fn describe(mods: CGEventFlags, keycode: Option<u16>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if mods.contains(CGEventFlags::CGEventFlagCommand) {
        parts.push("Cmd".into());
    }
    if mods.contains(CGEventFlags::CGEventFlagAlternate) {
        parts.push("Option".into());
    }
    if mods.contains(CGEventFlags::CGEventFlagControl) {
        parts.push("Ctrl".into());
    }
    if mods.contains(CGEventFlags::CGEventFlagShift) {
        parts.push("Shift".into());
    }
    if let Some(kc) = keycode {
        parts.push(keycode_to_name(kc));
    }
    if parts.is_empty() {
        "none".into()
    } else {
        parts.join("+")
    }
}

fn keycode_to_name(kc: u16) -> String {
    use core_graphics::event::KeyCode as K;
    match kc {
        v if v == K::SPACE => "Space".into(),
        v if v == K::RETURN => "Enter".into(),
        v if v == K::TAB => "Tab".into(),
        v if v == K::ESCAPE => "Esc".into(),
        v if v == K::F1 => "F1".into(),
        v if v == K::F2 => "F2".into(),
        v if v == K::F3 => "F3".into(),
        v if v == K::F4 => "F4".into(),
        v if v == K::F5 => "F5".into(),
        v if v == K::F6 => "F6".into(),
        v if v == K::F7 => "F7".into(),
        v if v == K::F8 => "F8".into(),
        v if v == K::F9 => "F9".into(),
        v if v == K::F10 => "F10".into(),
        v if v == K::F11 => "F11".into(),
        v if v == K::F12 => "F12".into(),
        _ => kc.to_string(),
    }
}

fn parse_modifiers(modifier: &str) -> CGEventFlags {
    let mut mods = CGEventFlags::empty();
    for raw in modifier.split('+') {
        match raw.trim().to_uppercase().as_str() {
            "OPTION" | "ALT" => mods |= CGEventFlags::CGEventFlagAlternate,
            "CONTROL" | "CTRL" => mods |= CGEventFlags::CGEventFlagControl,
            "COMMAND" | "CMD" | "SUPER" | "META" => mods |= CGEventFlags::CGEventFlagCommand,
            "SHIFT" => mods |= CGEventFlags::CGEventFlagShift,
            _ => {}
        }
    }
    mods
}

fn parse_key(key: &str) -> Result<Option<u16>> {
    let k = key.trim();
    if k.is_empty() {
        return Ok(None);
    }
    use core_graphics::event::KeyCode as K;
    let code = match k.to_uppercase().as_str() {
        "SPACE" => Some(K::SPACE),
        "ENTER" | "RETURN" => Some(K::RETURN),
        "TAB" => Some(K::TAB),
        "ESC" | "ESCAPE" => Some(K::ESCAPE),
        "DELETE" => Some(K::DELETE),
        "HOME" => Some(K::HOME),
        "END" => Some(K::END),
        "PAGEUP" => Some(K::PAGE_UP),
        "PAGEDOWN" => Some(K::PAGE_DOWN),
        "LEFT" | "ARROWLEFT" => Some(K::LEFT_ARROW),
        "RIGHT" | "ARROWRIGHT" => Some(K::RIGHT_ARROW),
        "UP" | "ARROWUP" => Some(K::UP_ARROW),
        "DOWN" | "ARROWDOWN" => Some(K::DOWN_ARROW),
        "F1" => Some(K::F1),
        "F2" => Some(K::F2),
        "F3" => Some(K::F3),
        "F4" => Some(K::F4),
        "F5" => Some(K::F5),
        "F6" => Some(K::F6),
        "F7" => Some(K::F7),
        "F8" => Some(K::F8),
        "F9" => Some(K::F9),
        "F10" => Some(K::F10),
        "F11" => Some(K::F11),
        "F12" => Some(K::F12),
        "0" | "DIGIT0" => Some(K::ANSI_0),
        "1" | "DIGIT1" => Some(K::ANSI_1),
        "2" | "DIGIT2" => Some(K::ANSI_2),
        "3" | "DIGIT3" => Some(K::ANSI_3),
        "4" | "DIGIT4" => Some(K::ANSI_4),
        "5" | "DIGIT5" => Some(K::ANSI_5),
        "6" | "DIGIT6" => Some(K::ANSI_6),
        "7" | "DIGIT7" => Some(K::ANSI_7),
        "8" | "DIGIT8" => Some(K::ANSI_8),
        "9" | "DIGIT9" => Some(K::ANSI_9),
        "A" => Some(K::ANSI_A),
        "B" => Some(K::ANSI_B),
        "C" => Some(K::ANSI_C),
        "D" => Some(K::ANSI_D),
        "E" => Some(K::ANSI_E),
        "F" => Some(K::ANSI_F),
        "G" => Some(K::ANSI_G),
        "H" => Some(K::ANSI_H),
        "I" => Some(K::ANSI_I),
        "J" => Some(K::ANSI_J),
        "K" => Some(K::ANSI_K),
        "L" => Some(K::ANSI_L),
        "M" => Some(K::ANSI_M),
        "N" => Some(K::ANSI_N),
        "O" => Some(K::ANSI_O),
        "P" => Some(K::ANSI_P),
        "Q" => Some(K::ANSI_Q),
        "R" => Some(K::ANSI_R),
        "S" => Some(K::ANSI_S),
        "T" => Some(K::ANSI_T),
        "U" => Some(K::ANSI_U),
        "V" => Some(K::ANSI_V),
        "W" => Some(K::ANSI_W),
        "X" => Some(K::ANSI_X),
        "Y" => Some(K::ANSI_Y),
        "Z" => Some(K::ANSI_Z),
        "`" | "BACKQUOTE" | "GRAVE" => Some(K::ANSI_GRAVE),
        "MINUS" | "-" => Some(K::ANSI_MINUS),
        "EQUAL" | "=" => Some(K::ANSI_EQUAL),
        "[" | "BRACKETLEFT" => Some(K::ANSI_LEFT_BRACKET),
        "]" | "BRACKETRIGHT" => Some(K::ANSI_RIGHT_BRACKET),
        "\\" | "BACKSLASH" => Some(K::ANSI_BACKSLASH),
        ";" | "SEMICOLON" => Some(K::ANSI_SEMICOLON),
        "'" | "QUOTE" => Some(K::ANSI_QUOTE),
        "," | "COMMA" => Some(K::ANSI_COMMA),
        "." | "PERIOD" => Some(K::ANSI_PERIOD),
        "/" | "SLASH" => Some(K::ANSI_SLASH),
        _ => None,
    };
    match code {
        Some(c) => Ok(Some(c)),
        None => anyhow::bail!("Unsupported hotkey key: {}", k),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_modifiers_shift_ctrl() {
        let m = parse_modifiers("Shift+Control");
        assert!(m.contains(CGEventFlags::CGEventFlagShift));
        assert!(m.contains(CGEventFlags::CGEventFlagControl));
        assert!(!m.contains(CGEventFlags::CGEventFlagCommand));
    }

    #[test]
    fn test_parse_modifiers_cmd_opt() {
        let m = parse_modifiers("Command+Option");
        assert!(m.contains(CGEventFlags::CGEventFlagCommand));
        assert!(m.contains(CGEventFlags::CGEventFlagAlternate));
    }

    #[test]
    fn test_parse_key_empty_is_modifier_only() {
        assert!(parse_key("").unwrap().is_none());
        assert!(parse_key("  ").unwrap().is_none());
    }

    #[test]
    fn test_parse_key_space() {
        let kc = parse_key("Space").unwrap().unwrap();
        use core_graphics::event::KeyCode as K;
        assert_eq!(kc, K::SPACE);
    }

    #[test]
    fn test_parse_key_unsupported() {
        assert!(parse_key("Fn").is_err());
    }
}
