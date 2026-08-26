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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppOverride;

    #[test]
    fn test_terminal_apps_use_type() {
        assert_eq!(classify_app("com.apple.Terminal"), InsertStrategy::Type);
        assert_eq!(classify_app("com.googlecode.iterm2"), InsertStrategy::Type);
        assert_eq!(classify_app("io.alacritty"), InsertStrategy::Type);
        assert_eq!(classify_app("net.kovidgoyal.kitty"), InsertStrategy::Type);
        assert_eq!(classify_app("dev.warp.Warp-Stable"), InsertStrategy::Type);
        assert_eq!(classify_app("com.mitchellh.ghostty"), InsertStrategy::Type);
    }

    #[test]
    fn test_electron_tuis_use_type() {
        assert_eq!(classify_app("com.anthropic.claudefordesktop"), InsertStrategy::Type);
        assert_eq!(classify_app("dev.opencode"), InsertStrategy::Type);
        assert_eq!(classify_app("com.openai.codex"), InsertStrategy::Type);
    }

    #[test]
    fn testIDES_use_paste() {
        assert_eq!(classify_app("com.microsoft.VSCode"), InsertStrategy::Paste);
        assert_eq!(classify_app("com.jetbrains.intellij"), InsertStrategy::Paste);
        assert_eq!(classify_app("com.jetbrains.rustrover"), InsertStrategy::Paste);
    }

    #[test]
    fn test_unknown_app_uses_paste() {
        assert_eq!(classify_app("com.spotify.client"), InsertStrategy::Paste);
        assert_eq!(classify_app("com.apple.Safari"), InsertStrategy::Paste);
    }

    #[test]
    fn test_none_bundle_id_uses_paste() {
        let config = InsertionConfig {
            default: "auto".into(),
            key_delay_ms: 20,
            paste_delay_ms: 10,
            restore_clipboard: true,
            apps: vec![],
        };
        assert_eq!(resolve_strategy(&None, &config), InsertStrategy::Paste);
    }

    #[test]
    fn test_user_override_takes_precedence() {
        let config = InsertionConfig {
            default: "auto".into(),
            key_delay_ms: 20,
            paste_delay_ms: 10,
            restore_clipboard: true,
            apps: vec![AppOverride {
                bundle_id: "com.anthropic.claudefordesktop".into(),
                strategy: "paste".into(),
            }],
        };
        assert_eq!(
            resolve_strategy(&Some("com.anthropic.claudefordesktop".into()), &config),
            InsertStrategy::Paste
        );
    }

    #[test]
    fn test_global_type_default() {
        let config = InsertionConfig {
            default: "type".into(),
            key_delay_ms: 20,
            paste_delay_ms: 10,
            restore_clipboard: true,
            apps: vec![],
        };
        assert_eq!(resolve_strategy(&None, &config), InsertStrategy::Type);
        assert_eq!(
            resolve_strategy(&Some("com.microsoft.VSCode".into()), &config),
            InsertStrategy::Type
        );
    }

    #[test]
    fn test_global_clipboard_only_default() {
        let config = InsertionConfig {
            default: "clipboard_only".into(),
            key_delay_ms: 20,
            paste_delay_ms: 10,
            restore_clipboard: true,
            apps: vec![],
        };
        assert_eq!(resolve_strategy(&None, &config), InsertStrategy::ClipboardOnly);
    }
}
