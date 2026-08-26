#![allow(dead_code)]

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::fs;

pub fn list() -> Result<()> {
    let model_dir = model_dir()?;

    println!("Whisper models (stored in {}):", model_dir.display());
    println!();

    let models = vec![
        ("tiny", "75 MB"),
        ("base", "142 MB"),
        ("small", "466 MB (recommended)"),
        ("medium", "1.5 GB"),
    ];

    for (name, desc) in models {
        let path = model_dir.join(format!("ggml-{}.bin", name));
        let status = if path.exists() {
            "✅ downloaded"
        } else {
            "   not downloaded"
        };
        println!("  {:>8}  {:<30}  {}", name, desc, status);
    }

    println!();
    println!("Run 'miccli models download <name>' to download a model.");
    Ok(())
}

pub fn download(name: &str) -> Result<()> {
    let valid = ["tiny", "base", "small", "medium"];
    if !valid.contains(&name) {
        anyhow::bail!("Invalid model '{}'. Choose from: tiny, base, small, medium", name);
    }

    let path = model_path(name)?;

    if path.exists() {
        println!("Model '{}' already downloaded at {}", name, path.display());
        return Ok(());
    }

    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin",
        name
    );

    println!("Downloading Whisper {} model...", name);
    println!("Source: {}", url);
    println!("Target: {}", path.display());

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client.get(&url).send().context("Download request failed")?;

    if !response.status().is_success() {
        anyhow::bail!("Download failed: HTTP {}", response.status());
    }

    let total_size = response.content_length().unwrap_or(0);
    println!("Size: {} MB", total_size / (1024 * 1024));

    let bytes = response.bytes().context("Failed to read download")?;

    fs::write(&path, &bytes).context("Failed to write model file")?;

    println!("✅ Model '{}' downloaded to {}", name, path.display());
    Ok(())
}

pub fn remove(name: &str) -> Result<()> {
    let path = model_path(name)?;

    if !path.exists() {
        println!("Model '{}' not found at {}", name, path.display());
        return Ok(());
    }

    fs::remove_file(&path).context("Failed to remove model file")?;
    println!("✅ Model '{}' removed", name);
    Ok(())
}

fn model_dir() -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .context("No home directory")?
        .join(".config")
        .join("miccli")
        .join("models");

    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn model_path(name: &str) -> Result<PathBuf> {
    Ok(model_dir()?.join(format!("ggml-{}.bin", name)))
}
