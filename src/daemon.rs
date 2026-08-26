use anyhow::{Context, Result};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::audio::AudioCapture;
use crate::cleanup;
use crate::config;
use crate::hotkey::HotkeyManager;
use crate::insert;
use crate::stt;

pub async fn start(_foreground: bool) -> Result<()> {
    let cfg = config::load_config()?;
    let running = Arc::new(AtomicBool::new(true));

    // Write PID file
    let pid_file = dirs::home_dir()
        .context("No home dir")?
        .join(".config")
        .join("miccli")
        .join("miccli.pid");
    fs::create_dir_all(pid_file.parent().unwrap())?;
    fs::write(&pid_file, std::process::id().to_string())?;

    // Ensure Whisper model is downloaded
    let model_path = stt::ensure_model(&cfg.whisper.model)?;
    tracing::info!("Whisper model: {}", model_path.display());

    // Initialize audio capture
    let audio = AudioCapture::new()?;
    tracing::info!(
        "Audio: {}Hz, {} ch",
        audio.sample_rate(),
        audio.channels()
    );

    let stt_engine = stt::WhisperStt::new(&model_path, &cfg.whisper.language, cfg.whisper.metal)?;

    // Set up hotkey
    let hotkey_manager = HotkeyManager::new(&cfg.hotkey.key, &cfg.hotkey.modifier, running.clone())?;

    println!("miccli listening... Hold {}+{} to talk", cfg.hotkey.modifier, cfg.hotkey.key);
    println!("   Press Ctrl+C to quit");
    println!();

    // Start audio capture
    let capture = AudioCapture::new()?;
    let audio_stream = capture.start_capture()?;
    let audio_rx = audio_stream.rx;

    // Main recording state
    let mut is_recording = false;
    let mut audio_buffer: Vec<f32> = Vec::new();

    // Main event loop
    while running.load(Ordering::Relaxed) {
        // Check for hotkey press
        if hotkey_manager.wait_for_press() {
            if is_recording {
                // Stop recording — process what we have
                is_recording = false;
                tracing::info!("Recording stopped ({} samples)", audio_buffer.len());

                if !audio_buffer.is_empty() {
                    let start = Instant::now();

                    // Transcribe
                    let raw_text = stt_engine.transcribe(&audio_buffer)?;
                    let transcribe_time = start.elapsed();

                    if raw_text.is_empty() {
                        tracing::info!("No speech detected");
                        continue;
                    }

                    tracing::info!("Raw: \"{}\" ({:.0?})", raw_text, transcribe_time);

                    // LLM cleanup (async call in sync context)
                    let cleanup_start = Instant::now();
                    let cleaned = cleanup::cleanup(&raw_text, &cfg.llm).await;
                    let cleanup_time = cleanup_start.elapsed();
                    tracing::info!("Cleaned: \"{}\" ({:.0?})", cleaned, cleanup_time);

                    // Insert text
                    let insert_start = Instant::now();
                    insert::insert_text(&cleaned, &cfg.insertion)?;
                    let insert_time = insert_start.elapsed();

                    let total = start.elapsed();
                    println!(
                        "\"{}\" ({:.0?} total: transcribe {:.0?} + cleanup {:.0?} + insert {:.0?})",
                        cleaned, total, transcribe_time, cleanup_time, insert_time
                    );

                    audio_buffer.clear();
                }
            } else {
                // Start recording
                is_recording = true;
                audio_buffer.clear();
                tracing::info!("Recording started...");
            }
        }

        // Drain any pending audio into buffer
        while let Ok(chunk) = audio_rx.try_recv() {
            if is_recording {
                audio_buffer.extend_from_slice(&chunk);
            }
        }

        thread::sleep(Duration::from_millis(5));
    }

    hotkey_manager.unregister()?;
    let _ = fs::remove_file(&pid_file);
    println!("miccli stopped.");
    Ok(())
}

pub fn send_signal(signal: &str) -> Result<()> {
    let pid_file = dirs::home_dir()
        .context("No home dir")?
        .join(".config")
        .join("miccli")
        .join("miccli.pid");

    if !pid_file.exists() {
        anyhow::bail!("miccli is not running (no PID file found)");
    }

    let pid_str = fs::read_to_string(&pid_file)?;
    let pid: i32 = pid_str.trim().parse().context("Invalid PID file")?;

    match signal {
        "toggle" => {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid, libc::SIGUSR1);
            }
            println!("Toggle signal sent to miccli (PID {})", pid);
        }
        "stop" => {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            println!("Stop signal sent to miccli (PID {})", pid);
        }
        _ => anyhow::bail!("Unknown signal: {}", signal),
    }

    Ok(())
}
