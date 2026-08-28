use anyhow::{Context, Result};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::audio::AudioCapture;
use crate::cleanup;
use crate::config;
use crate::hotkey::{HotkeyAction, HotkeyManager};
use crate::insert;
use crate::stt;
use crate::vad::{SileroVad, VadEvent};

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

    let stt_engine = stt::WhisperStt::new(&model_path, &cfg.whisper.language, cfg.whisper.metal)?;

    // Initialize Silero VAD
    let mut vad_engine = SileroVad::new(
        cfg.vad.threshold,
        cfg.vad.min_speech_ms,
        cfg.vad.min_silence_ms,
    )?;
    tracing::info!("Silero VAD loaded, threshold={}", cfg.vad.threshold);

    // Set up hotkey
    let hotkey_manager = HotkeyManager::new(&cfg.hotkey.key, &cfg.hotkey.modifier, running.clone())?;

    println!("miccli is running. Hold {} to talk, release to insert text.", hotkey_manager.combo());
    println!("   Press Ctrl+C to quit");
    println!("   ● appears while recording, ■ when stopped.");
    println!();
    // Keep status lines visible even when stdout is redirected.
    let _ = std::io::Write::flush(&mut std::io::stdout());

    // Start audio capture
    let capture = AudioCapture::new()?;
    tracing::info!("Audio: {}Hz, {} ch", capture.sample_rate(), capture.channels());
    let audio_stream = capture.start_capture()?;
    let audio_rx = audio_stream.rx;

    // Main recording state
    let mut is_recording = false;
    let mut audio_buffer: Vec<f32> = Vec::new();

    // Main hold-to-talk loop
    while running.load(Ordering::Relaxed) {
        // Drain any pending audio into the recording buffer
        while let Ok(chunk) = audio_rx.try_recv() {
            if is_recording {
                audio_buffer.extend_from_slice(&chunk);

                // Feed to VAD for auto-stop on silence
                match vad_engine.process(&chunk) {
                    Ok(VadEvent::SpeechEnd) => {
                        tracing::info!("VAD: speech ended, auto-stopping");
                        let buf = std::mem::take(&mut audio_buffer);
                        is_recording = false;
                        process_and_insert(
                            &buf,
                            &stt_engine,
                            &mut vad_engine,
                            &cfg.llm,
                            &cfg.insertion,
                        )
                        .await?;
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("VAD error: {}", e),
                }
            }
        }

        // Handle hotkey press/release (hold-to-talk)
        //
        // `Ok(None)` means no hotkey event arrived this poll — this is NOT a
        // release. We keep draining audio so a long hold records continuously
        // instead of stopping after one ~50ms poll.
        match hotkey_manager.wait_for_action() {
            Ok(Some(HotkeyAction::Pressed)) if !is_recording => {
                is_recording = true;
                audio_buffer.clear();
                print!("● recording…");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                tracing::info!("● recording started");
            }
            Ok(Some(HotkeyAction::Pressed)) => {
                // Already recording; ignore
            }
            Ok(Some(HotkeyAction::Released)) if is_recording => {
                is_recording = false;
                // The audio stream keeps pushing chunks into the channel while the
                // main thread was blocked waiting for the release. Drain those now
                // so nothing the user said during the hold is lost.
                while let Ok(chunk) = audio_rx.try_recv() {
                    audio_buffer.extend_from_slice(&chunk);
                }
                let n = audio_buffer.len();
                println!("■ stopped ({} samples)", n);
                tracing::info!("■ recording stopped ({} samples)", n);
                let buf = std::mem::take(&mut audio_buffer);
                process_and_insert(
                    &buf,
                    &stt_engine,
                    &mut vad_engine,
                    &cfg.llm,
                    &cfg.insertion,
                )
                .await?;
            }
            Ok(Some(HotkeyAction::Released)) => {
                // Not recording
            }
            Ok(None) => {
                // No hotkey event this poll; nothing to do, continue loop.
            }
            Err(()) => break,
        }
    }

    hotkey_manager.stop();
    let _ = fs::remove_file(&pid_file);
    println!("miccli stopped.");
    Ok(())
}

async fn process_and_insert(
    buffer: &[f32],
    stt_engine: &stt::WhisperStt,
    vad_engine: &mut SileroVad,
    llm: &config::LlmConfig,
    insertion: &config::InsertionConfig,
) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }

    // Gate on real speech using the Silero VAD. Ambient noise can trigger Whisper
    // to hallucinate phrases, so skip transcription/insertion unless actual speech
    // was detected.
    let verdict = vad_engine.contains_speech(buffer)?;
    println!(
        "   vad: speech={} max_prob={:.3} ({}/{} frames)",
        verdict.has_speech, verdict.max_prob, verdict.speech_frames, verdict.total_frames
    );
    if !verdict.has_speech {
        println!("   → no speech detected, no text inserted");
        tracing::info!(
            "No speech in buffer (max_prob={:.3}), skipping transcription",
            verdict.max_prob
        );
        return Ok(());
    }

    let start = Instant::now();

    let raw_text = stt_engine.transcribe(buffer)?;
    let transcribe_time = start.elapsed();

    if raw_text.is_empty() {
        println!("   → no speech detected by Whisper");
        tracing::info!("No speech detected");
        return Ok(());
    }

    tracing::info!("Raw: \"{}\" ({:.0?})", raw_text, transcribe_time);

    let cleanup_start = Instant::now();
    let cleaned = cleanup::cleanup(&raw_text, llm).await;
    let cleanup_time = cleanup_start.elapsed();
    tracing::info!("Cleaned: \"{}\" ({:.0?})", cleaned, cleanup_time);

    let insert_start = Instant::now();
    insert::insert_text(&cleaned, insertion)?;
    let insert_time = insert_start.elapsed();

    let total = start.elapsed();
    println!(
        "\"{}\" ({:.0?} total: transcribe {:.0?} + cleanup {:.0?} + insert {:.0?})",
        cleaned, total, transcribe_time, cleanup_time, insert_time
    );

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

#[cfg(test)]
mod tests {
    /// Records real mic audio and runs it through both the Silero VAD and Whisper
    /// to inspect the speech gate and STT pipeline with actual audio. Run manually:
    ///   cargo test --release record_and_transcribe -- --ignored --nocapture
    #[test]
    #[ignore]
    fn record_and_transcribe() {
        use crate::audio::AudioCapture;
        use crate::stt;
        use crate::vad::SileroVad;

        let audio = AudioCapture::new().expect("open audio");
        let stream = audio.start_capture().expect("start audio");
        let rx = stream.rx;

        let mut buf: Vec<f32> = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            while let Ok(chunk) = rx.try_recv() {
                buf.extend_from_slice(&chunk);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        println!("captured {} samples", buf.len());

        let mut vad = SileroVad::new(0.5, 250, 500).expect("vad");
        let verdict = vad.contains_speech(&buf).expect("score");
        println!(
            "VAD: has_speech={} max_prob={:.3} ({}/{} frames)",
            verdict.has_speech, verdict.max_prob, verdict.speech_frames, verdict.total_frames
        );

        let model = stt::ensure_model("small").expect("model");
        let engine = stt::WhisperStt::new(&model, "en", false).expect("stt");
        let text = engine.transcribe(&buf).expect("transcribe");
        println!("TRANSCRIBED: \"{}\"", text);
    }
}
