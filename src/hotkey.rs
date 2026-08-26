use anyhow::{Context, Result};
use global_hotkey::hotkey::HotKey;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct HotkeyManager {
    hotkey: HotKey,
    running: Arc<AtomicBool>,
}

impl HotkeyManager {
    pub fn new(key: &str, modifier: &str, running: Arc<AtomicBool>) -> Result<Self> {
        let hotkey_str = match (modifier, key) {
            ("Command", "Fn") => "Cmd+Fn",
            ("Command", "F5") => "Cmd+F5",
            ("Control", "Space") => "Ctrl+Space",
            _ => &format!("{}+{}", modifier, key),
        };

        let hotkey: HotKey = hotkey_str
            .parse()
            .with_context(|| format!("Invalid hotkey: {}", hotkey_str))?;

        let manager = global_hotkey::GlobalHotKeyManager::new()
            .context("Failed to create hotkey manager")?;

        manager.register(hotkey)
            .context("Failed to register hotkey")?;

        tracing::info!("Registered hotkey: {} (id={})", hotkey_str, hotkey.id());

        Ok(HotkeyManager { hotkey, running })
    }

    /// Blocks and returns when the hotkey is pressed. Returns true if toggle occurred.
    pub fn wait_for_press(&self) -> bool {
        let receiver = global_hotkey::GlobalHotKeyEvent::receiver();

        loop {
            if !self.running.load(Ordering::Relaxed) {
                return false;
            }

            if let Ok(event) = receiver.try_recv() {
                if event.id == self.hotkey.id() {
                    return true;
                }
            }

            thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn unregister(&self) -> Result<()> {
        let manager = global_hotkey::GlobalHotKeyManager::new()?;
        manager.unregister(self.hotkey)?;
        Ok(())
    }
}
