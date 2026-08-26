#![allow(dead_code)]

pub mod app_detect;
pub mod clipboard;
pub mod type_char;

use crate::config::InsertionConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum AppClass {
    Terminal,
    ElectronTui,
    Ide,
    Generic,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InsertStrategy {
    /// Slow character-by-character typing (safe for TUIs)
    Type,
    /// Fast clipboard paste (Cmd+V)
    Paste,
    /// Just copy to clipboard, user pastes manually
    ClipboardOnly,
}

pub fn insert_text(text: &str, config: &InsertionConfig) -> anyhow::Result<()> {
    let bundle_id = app_detect::get_frontmost_bundle_id();
    let strategy = resolve_strategy(&bundle_id, config);

    tracing::info!(
        "Inserting via {:?} (app: {:?})",
        strategy,
        bundle_id.as_deref().unwrap_or("unknown")
    );

    match strategy {
        InsertStrategy::Type => {
            type_char::type_text(text, config.key_delay_ms)?;
        }
        InsertStrategy::Paste => {
            clipboard::paste_text(text, config.paste_delay_ms, config.restore_clipboard)?;
        }
        InsertStrategy::ClipboardOnly => {
            clipboard::copy_to_clipboard(text)?;
            tracing::info!("Text copied to clipboard. Paste manually.");
        }
    }

    Ok(())
}

fn resolve_strategy(
    bundle_id: &Option<String>,
    config: &InsertionConfig,
) -> InsertStrategy {
    if config.default == "type" {
        return InsertStrategy::Type;
    }
    if config.default == "clipboard_only" {
        return InsertStrategy::ClipboardOnly;
    }

    if let Some(id) = bundle_id {
        // Check user overrides first
        for override_config in &config.apps {
            if &override_config.bundle_id == id {
                return match override_config.strategy.as_str() {
                    "type" => InsertStrategy::Type,
                    "paste" => InsertStrategy::Paste,
                    "clipboard_only" => InsertStrategy::ClipboardOnly,
                    _ => classify_app(id),
                };
            }
        }

        return classify_app(id);
    }

    // Default: fast paste
    InsertStrategy::Paste
}

fn classify_app(bundle_id: &str) -> InsertStrategy {
    match bundle_id {
        // Terminal emulators — slow typing (avoids paste collapse)
        "com.apple.Terminal"
        | "com.googlecode.iterm2"
        | "dev.warp.Warp-Stable"
        | "io.alacritty"
        | "net.kovidgoyal.kitty"
        | "com.github.wez.wezterm"
        | "com.mitchellh.ghostty"
        | "co.zeit.hyper" => InsertStrategy::Type,

        // Electron TUIs — slow typing (Ink/React raw mode)
        "com.anthropic.claudefordesktop"
        | "dev.opencode"
        | "com.openai.codex" => InsertStrategy::Type,

        // IDEs — fast paste works fine
        "com.microsoft.VSCode"
        | "com.jetbrains.intellij"
        | "com.jetbrains.rustrover"
        | "com.sublimetext.4"
        | "com.github.atom" => InsertStrategy::Paste,

        // Everything else — fast paste
        _ => InsertStrategy::Paste,
    }
}
