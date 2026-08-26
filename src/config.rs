use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct HotkeyConfig {
    #[serde(default = "default_hotkey_key")]
    pub key: String,
    #[serde(default = "default_hotkey_modifier")]
    pub modifier: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WhisperConfig {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_metal")]
    pub metal: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VadConfig {
    #[serde(default = "default_vad_threshold")]
    pub threshold: f32,
    #[serde(default = "default_min_speech_ms")]
    pub min_speech_ms: u32,
    #[serde(default = "default_min_silence_ms")]
    pub min_silence_ms: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LlmConfig {
    #[serde(default = "default_llm_provider")]
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct AppOverride {
    pub bundle_id: String,
    pub strategy: String,
}

fn default_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key: default_hotkey_key(),
        modifier: default_hotkey_modifier(),
    }
}
fn default_hotkey_key() -> String { "Fn".into() }
fn default_hotkey_modifier() -> String { "Command".into() }

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

fn default_llm() -> LlmConfig {
    LlmConfig {
        provider: default_llm_provider(),
        model: None,
        api_key_env: None,
        base_url: None,
        enabled: default_true(),
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
                enabled: default_true(),
            },
            insertion: default_insertion(),
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".config").join("miccli"))
}

pub fn load_config() -> Result<Config> {
    let config_dir = config_dir()?;
    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        tracing::info!("No config found at {}, using defaults", config_path.display());
        return Ok(Config::default());
    }

    let contents = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;

    let config: Config = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse {}", config_path.display()))?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_toml_uses_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.hotkey.key, "Fn");
        assert_eq!(config.hotkey.modifier, "Command");
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
        assert_eq!(config.hotkey.modifier, "Command");
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
        assert_eq!(config.hotkey.key, "Fn");
        assert_eq!(config.hotkey.modifier, "Command");
    }

    #[test]
    fn test_default_whisper_model() {
        let config = Config::default();
        assert_eq!(config.whisper.model, "small");
        assert_eq!(config.vad.threshold, 0.5);
    }
}
