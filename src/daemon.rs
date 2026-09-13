use anyhow::{Context, Result};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio::AudioCapture;
use crate::cleanup;
use crate::config;
use crate::hotkey::{HotkeyAction, HotkeyManager};
use crate::insert;
use crate::stt;
use crate::tui;
use crate::vad::{SileroVad, VadEvent};

pub async fn start(_foreground: bool, no_tui: bool) -> Result<()> {
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

    // Start audio capture
    let capture = AudioCapture::new()?;
    tracing::info!("Audio: {}Hz, {} ch", capture.sample_rate(), capture.channels());
    let audio_stream = capture.start_capture()?;
    let audio_rx = audio_stream.rx;

    let use_tui = !no_tui && tui::is_tty();

    let result = if use_tui {
        match tui::init_terminal() {
            Ok(mut terminal) => {
                let r = run_tui_loop(
                    &cfg,
                    &stt_engine,
                    &mut vad_engine,
                    &hotkey_manager,
                    audio_rx,
                    running.clone(),
                    &mut terminal,
                )
                .await;
                // Always restore even if loop errored
                let _ = tui::restore_terminal(&mut terminal);
                r
            }
            Err(e) => {
                eprintln!("TUI init failed ({}), falling back to plain mode", e);
                run_plain_loop(
                    &cfg,
                    &stt_engine,
                    &mut vad_engine,
                    &hotkey_manager,
                    audio_rx,
                    running.clone(),
                    &hotkey_manager.combo().to_string(),
                )
                .await
            }
        }
    } else {
        run_plain_loop(
            &cfg,
            &stt_engine,
            &mut vad_engine,
            &hotkey_manager,
            audio_rx,
            running.clone(),
            &hotkey_manager.combo().to_string(),
        )
        .await
    };

    hotkey_manager.stop();
    let _ = fs::remove_file(&pid_file);
    if use_tui {
        println!("miccli stopped.");
    } else {
        println!("miccli stopped.");
    }
    result
}

async fn run_plain_loop(
    cfg: &config::Config,
    stt_engine: &stt::WhisperStt,
    vad_engine: &mut SileroVad,
    hotkey_manager: &HotkeyManager,
    audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
    running: Arc<AtomicBool>,
    combo: &str,
) -> Result<()> {
    println!(
        "miccli is running. Hold {} to talk, release to insert text.",
        combo
    );
    println!("   Press Ctrl+C to quit");
    println!("   ● appears while recording, ■ when stopped.");
    println!();
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let mut is_recording = false;
    let mut audio_buffer: Vec<f32> = Vec::new();

    while running.load(Ordering::Relaxed) {
        while let Ok(chunk) = audio_rx.try_recv() {
            if is_recording {
                audio_buffer.extend_from_slice(&chunk);
                match vad_engine.process(&chunk) {
                    Ok(VadEvent::SpeechEnd) => {
                        tracing::info!("VAD: speech ended, auto-stopping");
                        let buf = std::mem::take(&mut audio_buffer);
                        is_recording = false;
                        process_and_insert(
                            &buf,
                            stt_engine,
                            vad_engine,
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

        match hotkey_manager.wait_for_action() {
            Ok(Some(HotkeyAction::Pressed)) if !is_recording => {
                is_recording = true;
                audio_buffer.clear();
                print!("● recording…");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                tracing::info!("● recording started");
            }
            Ok(Some(HotkeyAction::Pressed)) => {}
            Ok(Some(HotkeyAction::Released)) if is_recording => {
                is_recording = false;
                while let Ok(chunk) = audio_rx.try_recv() {
                    audio_buffer.extend_from_slice(&chunk);
                }
                let n = audio_buffer.len();
                println!("■ stopped ({} samples)", n);
                tracing::info!("■ recording stopped ({} samples)", n);
                let buf = std::mem::take(&mut audio_buffer);
                process_and_insert(&buf, stt_engine, vad_engine, &cfg.llm, &cfg.insertion).await?;
            }
            Ok(Some(HotkeyAction::Released)) => {}
            Ok(None) => {}
            Err(()) => break,
        }
    }
    Ok(())
}

async fn run_tui_loop(
    cfg: &config::Config,
    stt_engine: &stt::WhisperStt,
    vad_engine: &mut SileroVad,
    hotkey_manager: &HotkeyManager,
    audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
    running: Arc<AtomicBool>,
    terminal: &mut tui::TuiTerminal,
) -> Result<()> {
    use tui::{ui, WaveformHistory};

    let mut app = tui::build_initial_state(
        &hotkey_manager.combo(),
        &cfg.whisper.model,
        cfg.vad.threshold,
    );
    // Ensure insertion strategy shown matches config default
    app.strategy = cfg.insertion.default.clone();

    let mut waveform = WaveformHistory::new(120);
    let mut is_recording = false;
    let mut audio_buffer: Vec<f32> = Vec::new();
    let mut tick: usize = 0;
    let mut last_app_check = Instant::now();
    let mut pending_vad_stop = false;

    // Initial draw
    terminal.draw(|f| ui::render(f, &app, &waveform))?;

    while running.load(Ordering::Relaxed) {
        tick = tick.wrapping_add(1);
        app.tick = tick;

        // Adapt waveform capacity to terminal width
        if let Ok(size) = terminal.size() {
            let cap = (size.width as usize).saturating_sub(10).clamp(60, 220);
            waveform.set_capacity(cap);
        }

        // Periodically refresh frontmost app (throttled to avoid osascript spam)
        if last_app_check.elapsed() > Duration::from_millis(900) {
            last_app_check = Instant::now();
            let bundle = insert::app_detect::get_frontmost_bundle_id();
            app.app_name = bundle.clone();
            // Resolve strategy name for display
            let strat = resolve_strategy_name(&bundle, cfg);
            app.strategy = strat;
        }

        // Drain audio
        let mut had_chunk = false;
        let mut vad_triggered = false;
        while let Ok(chunk) = audio_rx.try_recv() {
            had_chunk = true;
            // Always push to waveform for live visualization
            waveform.push_chunk(&chunk);
            if is_recording {
                audio_buffer.extend_from_slice(&chunk);
                app.recording_samples = audio_buffer.len();
                // VAD for auto-stop
                match vad_engine.process(&chunk) {
                    Ok(VadEvent::SpeechEnd) => {
                        tracing::info!("VAD: speech ended, auto-stopping");
                        vad_triggered = true;
                        app.vad_status = "speech end → auto-stop".to_string();
                    }
                    Ok(VadEvent::SpeechStart) => {
                        app.vad_status = "speech detected".to_string();
                    }
                    Ok(VadEvent::None) => {
                        // keep last status
                    }
                    Err(e) => {
                        tracing::warn!("VAD error: {}", e);
                        app.vad_status = format!("vad err: {}", e);
                    }
                }
            }
        }

        if !had_chunk {
            // Add subtle decay to keep sparkline moving
            if tick % 3 == 0 {
                if is_recording {
                    // While recording silence, still push 0 so waveform dips
                    waveform.push_level(0);
                } else if tick % 6 == 0 {
                    waveform.push_silence();
                }
            }
        }

        // Handle VAD auto-stop after draining
        if vad_triggered && is_recording {
            pending_vad_stop = true;
        }

        if pending_vad_stop && is_recording {
            pending_vad_stop = false;
            is_recording = false;
            app.is_recording = false;
            app.status = "VAD auto-stop — transcribing…".to_string();
            // Drain any remaining audio that arrived while we processed VAD
            while let Ok(chunk) = audio_rx.try_recv() {
                audio_buffer.extend_from_slice(&chunk);
                waveform.push_chunk(&chunk);
            }
            app.recording_samples = audio_buffer.len();
            // Draw transcribing state before heavy work
            terminal.draw(|f| ui::render(f, &app, &waveform))?;
            let buf = std::mem::take(&mut audio_buffer);
            match process_and_insert_tui(&buf, stt_engine, vad_engine, &cfg.llm, &cfg.insertion).await {
                Ok(res) => {
                    if let Some(r) = res {
                        app.transcription = r.text.clone();
                        app.raw_text = r.raw.clone();
                        app.latencies = Some(r.latencies);
                        app.status = r.status;
                        app.error = r.error;
                        app.vad_status = r.vad_info;
                        if let Some(bundle) = r.bundle_id {
                            app.app_name = Some(bundle.clone());
                            app.strategy = resolve_strategy_name(&Some(bundle), cfg);
                        }
                    } else {
                        app.status = "No speech detected — try again".to_string();
                        app.vad_status = "no speech".to_string();
                    }
                }
                Err(e) => {
                    app.error = Some(e.to_string());
                    app.status = "Transcription failed".to_string();
                }
            }
            waveform.push_level(0);
        }

        // Hotkey handling (non-blocking)
        match hotkey_manager.try_action() {
            Ok(Some(HotkeyAction::Pressed)) if !is_recording => {
                is_recording = true;
                app.is_recording = true;
                audio_buffer.clear();
                app.recording_samples = 0;
                app.status = "● recording… hold to talk".to_string();
                app.error = None;
                app.vad_status = "listening…".to_string();
                waveform.clear();
                tracing::info!("● recording started (tui)");
            }
            Ok(Some(HotkeyAction::Pressed)) => {}
            Ok(Some(HotkeyAction::Released)) if is_recording => {
                is_recording = false;
                app.is_recording = false;
                while let Ok(chunk) = audio_rx.try_recv() {
                    audio_buffer.extend_from_slice(&chunk);
                    waveform.push_chunk(&chunk);
                }
                app.recording_samples = audio_buffer.len();
                app.status = format!("■ stopped ({} samples) — transcribing…", app.recording_samples);
                terminal.draw(|f| ui::render(f, &app, &waveform))?;
                let buf = std::mem::take(&mut audio_buffer);
                match process_and_insert_tui(&buf, stt_engine, vad_engine, &cfg.llm, &cfg.insertion).await {
                    Ok(res) => {
                        if let Some(r) = res {
                            app.transcription = r.text.clone();
                            app.raw_text = r.raw.clone();
                            app.latencies = Some(r.latencies);
                            app.status = r.status;
                            app.error = r.error;
                            app.vad_status = r.vad_info;
                            if let Some(bundle) = r.bundle_id {
                                app.app_name = Some(bundle.clone());
                                app.strategy = resolve_strategy_name(&Some(bundle), cfg);
                            }
                        } else {
                            app.status = "No speech detected — try again".to_string();
                            app.vad_status = "no speech".to_string();
                        }
                    }
                    Err(e) => {
                        app.error = Some(e.to_string());
                        app.status = "Transcription failed".to_string();
                    }
                }
            }
            Ok(Some(HotkeyAction::Released)) => {}
            Ok(None) => {}
            Err(()) => break,
        }

        // TUI key handling (non-blocking)
        match tui::poll_key_action(Duration::from_millis(0)) {
            Ok(tui::TuiKeyAction::Quit) => {
                running.store(false, Ordering::Relaxed);
                break;
            }
            Ok(tui::TuiKeyAction::NextTab) => app.next_tab(),
            Ok(tui::TuiKeyAction::PrevTab) => app.prev_tab(),
            Ok(tui::TuiKeyAction::SelectTab(i)) => app.set_tab(i),
            Ok(tui::TuiKeyAction::None) => {}
            Err(e) => {
                tracing::warn!("TUI key poll error: {}", e);
            }
        }

        // Render
        terminal.draw(|f| ui::render(f, &app, &waveform))?;

        // Small throttle to avoid busy spinning; keeps ~30fps while idle, instant while recording
        if is_recording {
            // While recording, loop fast for waveform smoothness
            tokio::time::sleep(Duration::from_millis(12)).await;
        } else {
            tokio::time::sleep(Duration::from_millis(33)).await;
        }
    }

    Ok(())
}

fn resolve_strategy_name(bundle: &Option<String>, cfg: &config::Config) -> String {
    if cfg.insertion.default == "type" {
        return "type".to_string();
    }
    if cfg.insertion.default == "clipboard_only" {
        return "clipboard_only".to_string();
    }
    if cfg.insertion.default == "clipboard" || cfg.insertion.default == "paste" {
        return "paste".to_string();
    }
    // auto: classify
    if let Some(id) = bundle {
        for ov in &cfg.insertion.apps {
            if &ov.bundle_id == id {
                return ov.strategy.clone();
            }
        }
        // Mirror insert::classify_app logic
        match id.as_str() {
            "com.apple.Terminal"
            | "com.googlecode.iterm2"
            | "dev.warp.Warp-Stable"
            | "io.alacritty"
            | "net.kovidgoyal.kitty"
            | "com.github.wez.wezterm"
            | "com.mitchellh.ghostty"
            | "co.zeit.hyper"
            | "com.anthropic.claudefordesktop"
            | "dev.opencode"
            | "com.openai.codex" => return "type".to_string(),
            "com.microsoft.VSCode"
            | "com.jetbrains.intellij"
            | "com.jetbrains.rustrover"
            | "com.sublimetext.4"
            | "com.github.atom" => return "paste".to_string(),
            _ => return "paste".to_string(),
        }
    }
    "paste".to_string()
}

struct TuiInsertResult {
    text: String,
    raw: String,
    latencies: tui::LatencyStats,
    status: String,
    error: Option<String>,
    vad_info: String,
    bundle_id: Option<String>,
}

async fn process_and_insert_tui(
    buffer: &[f32],
    stt_engine: &stt::WhisperStt,
    vad_engine: &mut SileroVad,
    llm: &config::LlmConfig,
    insertion: &config::InsertionConfig,
) -> Result<Option<TuiInsertResult>> {
    if buffer.is_empty() {
        return Ok(None);
    }

    let verdict = vad_engine.contains_speech(buffer)?;
    let vad_info = format!(
        "speech={} max{:.2} {}/{}",
        verdict.has_speech, verdict.max_prob, verdict.speech_frames, verdict.total_frames
    );
    if !verdict.has_speech {
        tracing::info!(
            "No speech in buffer (max_prob={:.3}), skipping",
            verdict.max_prob
        );
        return Ok(None);
    }

    let start = Instant::now();
    let raw_text = stt_engine.transcribe(buffer)?;
    let transcribe_time = start.elapsed();

    if raw_text.is_empty() {
        tracing::info!("No speech detected by Whisper");
        return Ok(None);
    }
    tracing::info!("Raw: \"{}\" ({:.0?})", raw_text, transcribe_time);

    let cleanup_start = Instant::now();
    let cleaned = cleanup::cleanup(&raw_text, llm).await;
    let cleanup_time = cleanup_start.elapsed();
    tracing::info!("Cleaned: \"{}\" ({:.0?})", cleaned, cleanup_time);

    let insert_start = Instant::now();
    // Capture bundle before insert for UI
    let bundle = insert::app_detect::get_frontmost_bundle_id();
    let insert_res = insert::insert_text(&cleaned, insertion);
    let insert_time = insert_start.elapsed();

    let total = start.elapsed();

    let (status, error) = match &insert_res {
        Ok(_) => (
            format!(
                "\"{}\" — {:.0?} (tr {:.0?} + cl {:.0?} + ins {:.0?})",
                cleaned, total, transcribe_time, cleanup_time, insert_time
            ),
            None,
        ),
        Err(e) => (
            format!("Inserted with error: {}", e),
            Some(e.to_string()),
        ),
    };
    // Even if insert failed, we still return text
    if let Err(e) = insert_res {
        tracing::warn!("insert failed: {}", e);
        // Don't bail, show error in UI
    }

    Ok(Some(TuiInsertResult {
        text: cleaned,
        raw: raw_text,
        latencies: tui::LatencyStats {
            transcribe: transcribe_time,
            cleanup: cleanup_time,
            insert: insert_time,
            total,
        },
        status,
        error,
        vad_info,
        bundle_id: bundle,
    }))
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
