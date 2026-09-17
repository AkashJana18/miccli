pub mod ui;
pub mod waveform;

pub use ui::{AppState, LatencyStats, ModelRow};
#[allow(unused_imports)]
pub use waveform::{level_to_block, waveform_to_blocks, WaveformHistory};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::IsTerminal;
use std::time::Duration;

/// Returns true if stdout is a TTY (can show TUI).
pub fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppMode {
    Overlay,
    Dashboard,
    None,
}

impl AppMode {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "overlay" | "minimal" | "hud" | "whisperflow" => AppMode::Overlay,
            "dashboard" | "full" | "tui" => AppMode::Dashboard,
            "none" | "off" | "plain" => AppMode::None,
            _ => AppMode::Overlay,
        }
    }
}

/// Actions derived from crossterm key events.
#[derive(Debug, Clone, Copy)]
pub enum TuiKeyAction {
    Quit,
    NextTab,
    PrevTab,
    SelectTab(usize),
    None,
}

pub fn poll_key_action(timeout: Duration) -> Result<TuiKeyAction> {
    if !event::poll(timeout)? {
        return Ok(TuiKeyAction::None);
    }
    let ev = event::read()?;
    Ok(match ev {
        Event::Key(k) => {
            // Ctrl+C always quits
            if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                return Ok(TuiKeyAction::Quit);
            }
            match k.code {
                KeyCode::Char('q') | KeyCode::Char('Q') => TuiKeyAction::Quit,
                KeyCode::Tab => {
                    if k.modifiers.contains(KeyModifiers::SHIFT) {
                        TuiKeyAction::PrevTab
                    } else {
                        TuiKeyAction::NextTab
                    }
                }
                KeyCode::BackTab => TuiKeyAction::PrevTab,
                KeyCode::Char('1') => TuiKeyAction::SelectTab(0),
                KeyCode::Char('2') => TuiKeyAction::SelectTab(1),
                KeyCode::Char('3') => TuiKeyAction::SelectTab(2),
                KeyCode::Char('4') => TuiKeyAction::SelectTab(3),
                KeyCode::Esc => TuiKeyAction::Quit,
                _ => TuiKeyAction::None,
            }
        }
        _ => TuiKeyAction::None,
    })
}

pub type TuiTerminal = Terminal<CrosstermBackend<std::io::Stdout>>;

/// Full dashboard: alt-screen + clear
pub fn init_terminal() -> Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

/// Overlay (whisperflow): alt-screen + clear only while recording — when idle we stay on primary screen.
pub fn init_overlay_terminal() -> Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

pub fn restore_terminal(terminal: &mut TuiTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

#[allow(dead_code)]
pub fn wants_overlay(cfg: &crate::config::Config) -> bool {
    AppMode::from_str(&cfg.tui.mode) == AppMode::Overlay && is_tty()
}
#[allow(dead_code)]
pub fn wants_dashboard(cfg: &crate::config::Config) -> bool {
    AppMode::from_str(&cfg.tui.mode) == AppMode::Dashboard && is_tty()
}

/// Build initial AppState from config + filesystem.
pub fn build_initial_state(hotkey: &str, model_name: &str, vad_threshold: f32) -> AppState {
    use std::path::PathBuf;

    let config_path = crate::config::config_dir()
        .map(|p| p.join("config.toml").display().to_string())
        .unwrap_or_else(|_| "~/.config/miccli/config.toml".to_string());

    let config_file = crate::config::config_dir()
        .map(|p| p.join("config.toml"))
        .unwrap_or_else(|_| PathBuf::from("~/.config/miccli/config.toml"));
    let config_text = std::fs::read_to_string(&config_file)
        .unwrap_or_else(|_| {
            "# No config file yet — using defaults.\n# Edit ~/.config/miccli/config.toml to customize.\n".to_string()
                + &default_config_text()
        });

    let model_dir = crate::config::config_dir()
        .map(|p| p.join("models"))
        .unwrap_or_else(|_| PathBuf::from("~/.config/miccli/models"));

    let model_rows = vec![
        ModelRow {
            name: "tiny",
            size: "75 MB",
            installed: model_dir.join("ggml-tiny.bin").exists(),
            path: model_dir.join("ggml-tiny.bin").display().to_string(),
        },
        ModelRow {
            name: "base",
            size: "142 MB",
            installed: model_dir.join("ggml-base.bin").exists(),
            path: model_dir.join("ggml-base.bin").display().to_string(),
        },
        ModelRow {
            name: "small",
            size: "466 MB",
            installed: model_dir.join("ggml-small.bin").exists(),
            path: model_dir.join("ggml-small.bin").display().to_string(),
        },
        ModelRow {
            name: "medium",
            size: "1.5 GB",
            installed: model_dir.join("ggml-medium.bin").exists(),
            path: model_dir.join("ggml-medium.bin").display().to_string(),
        },
    ];

    let model_installed = model_rows
        .iter()
        .find(|r| r.name == model_name)
        .map(|r| r.installed)
        .unwrap_or(false);

    AppState {
        hotkey: hotkey.to_string(),
        model_name: model_name.to_string(),
        model_installed,
        vad_threshold,
        models: model_rows,
        config_text,
        config_path,
        ..Default::default()
    }
}

fn default_config_text() -> String {
    r#"[hotkey]
key = ""                      # leave empty for modifier-only hold
modifier = "Shift+Control"

[whisper]
model = "small"
language = "en"
metal = true

[vad]
threshold = 0.5
min_speech_ms = 250
min_silence_ms = 500

[llm]
provider = "ollama"
model = "qwen2.5:1.5b"
enabled = false               # opt-in: 1=disabled, 2=ollama local, 3=groq BYOK, 4=openai BYOK

[insertion]
default = "auto"
key_delay_ms = 20
paste_delay_ms = 10
restore_clipboard = true

[tui]
mode = "overlay"              # overlay | dashboard | none
"#
    .to_string()
}
