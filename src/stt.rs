use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct WhisperStt {
    context: WhisperContext,
    language: String,
}

impl WhisperStt {
    pub fn new(model_path: &Path, language: &str, _use_metal: bool) -> Result<Self> {
        let params = WhisperContextParameters::default();

        let context = WhisperContext::new_with_params(
            model_path.to_str().context("Invalid model path")?,
            params,
        )
        .context("Failed to load whisper model")?;

        tracing::info!(
            "Loaded whisper model: {} (lang={})",
            model_path.display(),
            language
        );

        Ok(WhisperStt {
            context,
            language: language.to_string(),
        })
    }

    pub fn transcribe(&self, audio: &[f32]) -> Result<String> {
        let mut state = self.context.create_state()
            .context("Failed to create whisper state")?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_token_timestamps(false);

        state.full(params, audio)
            .context("Whisper transcription failed")?;

        let num_segments = state.full_n_segments();

        let mut text = String::new();
        for i in 0..num_segments {
            if let Some(segment) = state.get_segment(i) {
                if let Ok(segment_text) = segment.to_str_lossy() {
                    text.push_str(&segment_text);
                }
            }
        }

        Ok(text.trim().to_string())
    }
}

pub fn model_path(model_name: &str) -> Result<PathBuf> {
    let model_dir = dirs::home_dir()
        .context("No home dir")?
        .join(".config")
        .join("miccli")
        .join("models");

    Ok(model_dir.join(format!("ggml-{}.bin", model_name)))
}

pub fn ensure_model(name: &str) -> Result<PathBuf> {
    let path = model_path(name)?;

    if !path.exists() {
        tracing::info!("Downloading Whisper {} model...", name);
        download_whisper_model(name, &path)?;
        tracing::info!("Model saved to {}", path.display());
    }

    Ok(path)
}

fn download_whisper_model(name: &str, path: &Path) -> Result<()> {
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin",
        name
    );
    tracing::info!("Downloading from: {}", url);

    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?
        .get(&url)
        .send()
        .context("Download failed")?;

    let bytes = response.bytes().context("Failed to read response")?;
    std::fs::write(path, &bytes).context("Failed to write model")?;

    Ok(())
}

pub fn list_models() -> Vec<(&'static str, &'static str, bool)> {
    vec![
        ("tiny", "75 MB - Fastest, lowest accuracy", false),
        ("base", "142 MB - Good balance", false),
        ("small", "466 MB - Best for dictation (recommended)", true),
        ("medium", "1.5 GB - High accuracy, slower", false),
    ]
}
