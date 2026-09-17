use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_hotkey")]
    pub hotkey: HotkeyConfig,
    #[serde(default = "default_whisper")]
    pub whisper: WhisperConfig,
    #[serde(default = "default_vad")]
    pub vad: VadConfig,
    #[serde(default = "default_llm")]
    pub llm: LlmConfig,
    #[serde(default = "default_insertion")]
    pub insertion: InsertionConfig,
    #[serde(default = "default_tui")]
    pub tui: TuiConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HotkeyConfig {
    #[serde(default = "default_hotkey_key")]
    pub key: String,
    #[serde(default = "default_hotkey_modifier")]
    pub modifier: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WhisperConfig {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_metal")]
    pub metal: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VadConfig {
    #[serde(default = "default_vad_threshold")]
    pub threshold: f32,
    #[serde(default = "default_min_speech_ms")]
    pub min_speech_ms: u32,
    #[serde(default = "default_min_silence_ms")]
    pub min_silence_ms: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LlmConfig {
    #[serde(default = "default_llm_provider")]
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_llm_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InsertionConfig {
    #[serde(default = "default_insertion_strategy")]
    pub default: String,
    #[serde(default = "default_key_delay_ms")]
    pub key_delay_ms: u64,
    #[serde(default = "default_paste_delay_ms")]
    pub paste_delay_ms: u64,
    #[serde(default = "default_true")]
    pub restore_clipboard: bool,
    #[serde(default)]
    pub apps: Vec<AppOverride>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppOverride {
    pub bundle_id: String,
    pub strategy: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TuiConfig {
    #[serde(default = "default_tui_mode")]
    pub mode: String,
}

fn default_tui() -> TuiConfig {
    TuiConfig {
        mode: default_tui_mode(),
    }
}
fn default_tui_mode() -> String {
    "overlay".into()
}

fn default_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key: default_hotkey_key(),
        modifier: default_hotkey_modifier(),
    }
}
fn default_hotkey_key() -> String { "".into() }
fn default_hotkey_modifier() -> String { "Shift+Control".into() }

fn default_whisper() -> WhisperConfig {
    WhisperConfig {
        model: default_model(),
        language: default_language(),
        metal: default_metal(),
    }
}
fn default_model() -> String { "small".into() }
fn default_language() -> String { "en".into() }
fn default_metal() -> bool { true }

fn default_vad() -> VadConfig {
    VadConfig {
        threshold: default_vad_threshold(),
        min_speech_ms: default_min_speech_ms(),
        min_silence_ms: default_min_silence_ms(),
    }
}
fn default_vad_threshold() -> f32 { 0.5 }
fn default_min_speech_ms() -> u32 { 250 }
fn default_min_silence_ms() -> u32 { 500 }

fn default_llm_provider() -> String { "ollama".into() }
fn default_true() -> bool { true }
fn default_llm_enabled() -> bool { false }

fn default_llm() -> LlmConfig {
    LlmConfig {
        provider: default_llm_provider(),
        model: None,
        api_key_env: None,
        base_url: None,
        enabled: default_llm_enabled(),
    }
}

fn default_insertion() -> InsertionConfig {
    InsertionConfig {
        default: default_insertion_strategy(),
        key_delay_ms: default_key_delay_ms(),
        paste_delay_ms: default_paste_delay_ms(),
        restore_clipboard: default_true(),
        apps: vec![],
    }
}
fn default_insertion_strategy() -> String { "auto".into() }
fn default_key_delay_ms() -> u64 { 20 }
fn default_paste_delay_ms() -> u64 { 10 }

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: default_hotkey(),
            whisper: default_whisper(),
            vad: default_vad(),
            llm: LlmConfig {
                provider: default_llm_provider(),
                model: None,
                api_key_env: None,
                base_url: None,
                enabled: default_llm_enabled(),
            },
            insertion: default_insertion(),
            tui: default_tui(),
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    // Respect XDG_CONFIG_HOME / platform config dir; keep ~/.config/miccli for
    // backwards compat on macOS (where dirs::config_dir is ~/Library/Application Support).
    if let Some(base) = dirs::config_dir() {
        #[cfg(target_os = "macos")]
        {
            if std::env::var_os("XDG_CONFIG_HOME").is_none() {
                if let Some(home) = dirs::home_dir() {
                    let legacy = home.join(".config").join("miccli");
                    // Prefer legacy ~/.config/miccli if it already exists or the new
                    // location doesn't yet exist — preserves existing user configs.
                    if legacy.exists() || !base.join("miccli").exists() {
                        return Ok(legacy);
                    }
                }
            }
        }
        Ok(base.join("miccli"))
    } else {
        let home = dirs::home_dir().context("Could not determine home or config directory")?;
        Ok(home.join(".config").join("miccli"))
    }
}

pub fn pid_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("miccli.pid"))
}

pub fn log_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("miccli.log"))
}

pub fn load_config() -> Result<Config> {
    let config_dir = config_dir()?;
    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        // First-run: offer LLM opt-in if interactive (TTY) and not in background daemon
        if std::io::IsTerminal::is_terminal(&std::io::stdin())
            && std::io::IsTerminal::is_terminal(&std::io::stderr())
            && std::env::var_os("MICCLI_NO_PROMPT").is_none()
        {
            if let Some(chosen_llm) = prompt_llm_setup()? {
                let cfg = Config { llm: chosen_llm, ..Default::default() };
                // Persist the choice so next run doesn't re-prompt
                if let Err(e) = write_config(&cfg) {
                    tracing::warn!("Failed to write first-run config: {}", e);
                } else {
                    tracing::info!("Wrote first-run config to {}", config_path.display());
                }
                return Ok(cfg);
            }
        }
        tracing::info!("No config found at {}, using defaults (LLM disabled, opt-in)", config_path.display());
        // Write default config for discoverability (opt-in false)
        let default_cfg = Config::default();
        let _ = write_config(&default_cfg);
        return Ok(default_cfg);
    }

    let contents = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;

    let config: Config = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse {}", config_path.display()))?;

    Ok(config)
}

fn write_config(cfg: &Config) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir).context("Failed to create config dir")?;
    let path = dir.join("config.toml");
    let toml_str = toml::to_string(cfg).context("Failed to serialize config")?;
    let header = "# miccli config — edit and restart daemon (`miccli restart`) to apply\n\
                  # LLM cleanup is opt-in (enabled = false by default). Enable via prompt or set enabled = true\n";
    std::fs::write(&path, header.to_string() + &toml_str).context("Failed to write config")?;
    Ok(())
}

fn prompt_llm_setup() -> Result<Option<LlmConfig>> {
    use std::io::{self, Write};
    // Don't prompt in CI / non-interactive envs
    if std::env::var_os("CI").is_some() {
        return Ok(None);
    }
    eprintln!();
    eprintln!("miccli first run — LLM cleanup is disabled by default (opt-in).");
    eprintln!("Enable AI-powered punctuation/filler cleanup?");
    eprintln!("  1) No — keep disabled (default, fastest, offline)");
    eprintln!("  2) Ollama — local, free (requires `ollama pull qwen2.5:1.5b`)");
    eprintln!("  3) Groq — cloud, BYOK (requires GROQ_API_KEY env)");
    eprintln!("  4) OpenAI — cloud, BYOK (requires OPENAI_API_KEY env)");
    eprint!("Choice [1-4, default 1]: ");
    let _ = io::stderr().flush();
    // Spawn thread to avoid hanging background daemon forever (30s timeout)
    let input = {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = io::stdin().read_line(&mut s);
            let _ = tx.send(s);
        });
        rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap_or_default()
    };
    let choice = input.trim();
    let llm = match choice {
        "2" => LlmConfig {
            provider: "ollama".into(),
            model: Some("qwen2.5:1.5b".into()),
            api_key_env: None,
            base_url: None,
            enabled: true,
        },
        "3" => LlmConfig {
            provider: "groq".into(),
            model: Some("llama-3.1-8b-instant".into()),
            api_key_env: Some("GROQ_API_KEY".into()),
            base_url: None,
            enabled: true,
        },
        "4" => LlmConfig {
            provider: "openai".into(),
            model: Some("gpt-4o-mini".into()),
            api_key_env: Some("OPENAI_API_KEY".into()),
            base_url: None,
            enabled: true,
        },
        _ => return Ok(None), // 1, empty, or invalid → disabled
    };
    eprintln!("✓ LLM enabled: provider={}, model={}", llm.provider, llm.model.as_deref().unwrap_or("default"));
    if llm.provider == "groq" || llm.provider == "openai" {
        eprintln!("  Set {} env var before running `miccli start`", llm.api_key_env.as_deref().unwrap_or("API_KEY"));
    } else if llm.provider == "ollama" {
        eprintln!("  Run: ollama pull qwen2.5:1.5b  (if not already)");
    }
    Ok(Some(llm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_toml_uses_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.hotkey.key, "");
        assert_eq!(config.hotkey.modifier, "Shift+Control");
        assert_eq!(config.whisper.model, "small");
        assert_eq!(config.whisper.language, "en");
        assert_eq!(config.llm.provider, "ollama");
        assert_eq!(config.insertion.key_delay_ms, 20);
        assert!(config.insertion.restore_clipboard);
    }

    #[test]
    fn test_partial_config() {
        let toml = r#"
[hotkey]
key = "F5"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.hotkey.key, "F5");
        assert_eq!(config.hotkey.modifier, "Shift+Control");
        assert_eq!(config.whisper.model, "small");
    }

    #[test]
    fn test_full_config_with_overrides() {
        let toml = r#"
[hotkey]
key = "Space"
modifier = "Control"

[whisper]
model = "medium"
language = "es"
metal = false

[llm]
provider = "groq"
model = "llama-3.1-8b-instant"
enabled = false

[insertion]
default = "type"
key_delay_ms = 30

[[insertion.apps]]
bundle_id = "com.anthropic.claudefordesktop"
strategy = "paste"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.hotkey.key, "Space");
        assert_eq!(config.whisper.model, "medium");
        assert_eq!(config.whisper.language, "es");
        assert!(!config.whisper.metal);
        assert_eq!(config.llm.provider, "groq");
        assert!(!config.llm.enabled);
        assert_eq!(config.insertion.default, "type");
        assert_eq!(config.insertion.key_delay_ms, 30);
        assert_eq!(config.insertion.apps.len(), 1);
        assert_eq!(config.insertion.apps[0].bundle_id, "com.anthropic.claudefordesktop");
        assert_eq!(config.insertion.apps[0].strategy, "paste");
    }

    #[test]
    fn test_invalid_toml() {
        let result = toml::from_str::<Config>("this is not toml [[[");
        assert!(result.is_err());
    }

    #[test]
    fn test_default_hotkey_values() {
        let config = Config::default();
        assert_eq!(config.hotkey.key, "");
        assert_eq!(config.hotkey.modifier, "Shift+Control");
    }

    #[test]
    fn test_default_whisper_model() {
        let config = Config::default();
        assert_eq!(config.whisper.model, "small");
        assert_eq!(config.vad.threshold, 0.5);
    }

    #[test]
    fn test_default_tui_mode() {
        let config = Config::default();
        assert_eq!(config.tui.mode, "overlay");
        let parsed: Config = toml::from_str("[tui]\nmode = \"dashboard\"").unwrap();
        assert_eq!(parsed.tui.mode, "dashboard");
        let parsed_none: Config = toml::from_str("[tui]\nmode = \"none\"").unwrap();
        assert_eq!(parsed_none.tui.mode, "none");
    }
}
