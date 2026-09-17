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
use crate::overlay;
use crate::stt;
use crate::tui;
use crate::vad::{SileroVad, VadEvent};

pub fn pid_file_path() -> Result<std::path::PathBuf> {
    config::pid_file_path()
}

fn default_log_path() -> Result<std::path::PathBuf> {
    config::log_file_path()
}

#[cfg(unix)]
fn is_process_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(not(unix))]
fn is_process_alive(_pid: i32) -> bool {
    false
}

struct PidFileGuard {
    path: std::path::PathBuf,
}
impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub async fn start() -> Result<()> {
    let cfg = config::load_config()?;
    let running = Arc::new(AtomicBool::new(true));
    let paused = Arc::new(AtomicBool::new(false));

    // Signal handlers for SIGUSR1 (toggle) and SIGTERM/SIGINT (graceful stop)
    setup_pause_handler(paused.clone());
    setup_termination_handler(running.clone());

    // PID file with duplicate-instance protection
    let pid_file = pid_file_path()?;
    // Check for stale or running instance before writing
    if pid_file.exists() {
        if let Ok(pid_str) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                if is_process_alive(pid) {
                    anyhow::bail!(
                        "miccli is already running (PID {}), use `miccli stop` or `miccli restart` or `miccli status`",
                        pid
                    );
                } else {
                    tracing::warn!("Removing stale PID file (PID {} not running)", pid);
                    let _ = fs::remove_file(&pid_file);
                }
            } else {
                let _ = fs::remove_file(&pid_file);
            }
        }
    }
    fs::create_dir_all(pid_file.parent().context("PID file has no parent")?)?;
    fs::write(&pid_file, std::process::id().to_string())?;
    let _pid_guard = PidFileGuard {
        path: pid_file.clone(),
    };
    // Ensure PID file removed even on panic (unwind)
    let pid_for_hook = pid_file.clone();
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = fs::remove_file(&pid_for_hook);
        prev_hook(info);
    }));

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

    let mode = tui::AppMode::from_str(&cfg.tui.mode);
    let is_tty = tui::is_tty();

    let result = match mode {
        tui::AppMode::None => {
            // Plain logs (tui.mode = none)
            run_plain_loop(
                &cfg,
                &stt_engine,
                &mut vad_engine,
                &hotkey_manager,
                audio_rx,
                running.clone(),
                paused.clone(),
                hotkey_manager.combo(),
            )
            .await
        }
        tui::AppMode::Dashboard if is_tty => {
            // Persistent full dashboard as daemon (if user sets tui.mode=dashboard and TTY)
            match tui::init_terminal() {
                Ok(mut terminal) => {
                    let r = run_dashboard_loop(
                        &cfg,
                        &stt_engine,
                        &mut vad_engine,
                        &hotkey_manager,
                        audio_rx,
                        running.clone(),
                        paused.clone(),
                        &mut terminal,
                    )
                    .await;
                    let _ = tui::restore_terminal(&mut terminal);
                    r
                }
                Err(e) => {
                    eprintln!("TUI init failed ({}), falling back to plain", e);
                    run_plain_loop(
                        &cfg,
                        &stt_engine,
                        &mut vad_engine,
                        &hotkey_manager,
                        audio_rx,
                        running.clone(),
                        paused.clone(),
                        hotkey_manager.combo(),
                    )
                    .await
                }
            }
        }
        tui::AppMode::Dashboard => {
            // Dashboard Fallback: not a TTY, use plain logs
            tracing::info!("tui.mode=dashboard but not a TTY, falling back to plain logs");
            run_plain_loop(
                &cfg,
                &stt_engine,
                &mut vad_engine,
                &hotkey_manager,
                audio_rx,
                running.clone(),
                paused.clone(),
                hotkey_manager.combo(),
            )
            .await
        }
        tui::AppMode::Overlay => {
            // Whisperflow-style: overlay is global (NSWindow) and independent of terminal TTY.
            // Run overlay even when not a TTY (background daemon with stdout redirected to log).
            // Falls back to plain only if overlay creation fails internally.
            run_overlay_loop(
                &cfg,
                &stt_engine,
                &mut vad_engine,
                &hotkey_manager,
                audio_rx,
                running.clone(),
                paused.clone(),
            )
            .await
        }
    };

    hotkey_manager.stop();
    let _ = fs::remove_file(&pid_file);
    println!("miccli stopped.");
    result
}

/// Dashboard command: stop daemon if running → show full 4-tab dashboard → auto-restart daemon
pub async fn dashboard() -> Result<()> {
    let pid_file = config::pid_file_path()?;
    let was_running = pid_file.exists();
    if was_running {
        println!("Stopping running miccli daemon...");
        let _ = send_signal("stop");
        // Wait for pid file removal (daemon exit + restore_terminal)
        for _ in 0..50 {
            if !pid_file.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // Extra grace to let old terminal restore
        tokio::time::sleep(Duration::from_millis(300)).await;
        if pid_file.exists() {
            eprintln!("Warning: daemon pid file still exists, dashboard may conflict");
        } else {
            println!("Daemon stopped. Opening dashboard...");
        }
    }

    // Run full dashboard (standalone, no audio/hotkey needed — just config + models)
    let cfg = config::load_config().unwrap_or_default();
    // Try to init dashboard terminal
    let res = match tui::init_terminal() {
        Ok(mut terminal) => {
            let r = run_dashboard_standalone(&cfg, &mut terminal).await;
            let _ = tui::restore_terminal(&mut terminal);
            r
        }
        Err(e) => {
            eprintln!("Dashboard TUI failed ({}), showing plain info", e);
            // Fallback plain
            let cfg_dir = config::config_dir()?;
            println!("Config: {}", cfg_dir.join("config.toml").display());
            println!("TUI mode: {}", cfg.tui.mode);
            println!("Run `miccli models list` or edit config.toml");
            Ok(())
        }
    };

    if was_running {
        println!("\nDashboard closed — restarting miccli daemon (overlay)...");
        // Become the new daemon (overlay) in this terminal.
        // This blocks until the new daemon is stopped.
        // If user wants background, they can Ctrl+Z / or run in another pane.
        // We re-enter start() which will run overlay loop.
        // Note: start() will re-write pid file.
        if let Err(e) = start().await {
            eprintln!("Failed to restart daemon: {}", e);
            return res;
        }
    }
    res
}

async fn run_dashboard_standalone(cfg: &config::Config, terminal: &mut tui::TuiTerminal) -> Result<()> {
    use tui::{ui, WaveformHistory};
    let mut app = tui::build_initial_state("Shift+Control", &cfg.whisper.model, cfg.vad.threshold);
    app.mode = tui::AppMode::Dashboard;
    app.strategy = cfg.insertion.default.clone();
    // Show dashboard until q
    let waveform = WaveformHistory::new(120);
    // Pre-fill a tiny waveform for demo
    // No live audio in dashboard standalone — just static

    terminal.draw(|f| ui::render_dashboard(f, &app, &waveform))?;

    loop {
        if let Ok(action) = tui::poll_key_action(Duration::from_millis(80)) {
            match action {
                tui::TuiKeyAction::Quit => break,
                tui::TuiKeyAction::NextTab => app.next_tab(),
                tui::TuiKeyAction::PrevTab => app.prev_tab(),
                tui::TuiKeyAction::SelectTab(i) => app.set_tab(i),
                tui::TuiKeyAction::None => {}
            }
        }
        // Keep app tick for pulsing if needed
        app.tick = app.tick.wrapping_add(1);
        terminal.draw(|f| ui::render_dashboard(f, &app, &waveform))?;
        tokio::time::sleep(Duration::from_millis(33)).await;
    }
    Ok(())
}

fn setup_pause_handler(paused: Arc<AtomicBool>) {
    #[cfg(unix)]
    {
        // Use tokio::signal for SIGUSR1 if available; fallback to safe no-op if not.
        // We spawn a background task that toggles `paused` on SIGUSR1.
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                if let Ok(mut sig) = signal(SignalKind::user_defined1()) {
                    while sig.recv().await.is_some() {
                        let prev = paused.load(Ordering::Relaxed);
                        paused.store(!prev, Ordering::Relaxed);
                        if !prev {
                            tracing::info!("⏸ paused (toggle)");
                        } else {
                            tracing::info!("▶ resumed (toggle)");
                        }
                    }
                }
            }
        });
    }
    #[cfg(not(unix))]
    {
        let _ = paused;
    }
}

fn setup_termination_handler(running: Arc<AtomicBool>) {
    #[cfg(unix)]
    {
        let r1 = running.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
                    while sigterm.recv().await.is_some() {
                        tracing::info!("SIGTERM received, stopping daemon");
                        r1.store(false, Ordering::Relaxed);
                        // Break after first, but keep loop for potential second signal to force exit
                        // Next recv will block until second SIGTERM, if daemon hasn't exited yet
                        // we keep handling but running already false
                    }
                }
            }
        });
        let r2 = running.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                if let Ok(mut sigint) = signal(SignalKind::interrupt()) {
                    while sigint.recv().await.is_some() {
                        tracing::info!("SIGINT received, stopping daemon");
                        r2.store(false, Ordering::Relaxed);
                    }
                }
            }
        });
    }
    #[cfg(not(unix))]
    {
        let _ = running;
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_plain_loop(
    cfg: &config::Config,
    stt_engine: &stt::WhisperStt,
    vad_engine: &mut SileroVad,
    hotkey_manager: &HotkeyManager,
    audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    combo: &str,
) -> Result<()> {
    println!(
        "miccli is running (overlay={}). Hold {} to talk, release to insert text.",
        cfg.tui.mode, combo
    );
    if paused.load(Ordering::Relaxed) {
        println!("   ⏸ paused — `miccli toggle` to resume");
    }
    println!("   Press Ctrl+C to quit, `miccli toggle` to pause/resume, `miccli dashboard` for full TUI");
    println!();
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let mut is_recording = false;
    let mut audio_buffer: Vec<f32> = Vec::new();

    while running.load(Ordering::Relaxed) {
        while let Ok(chunk) = audio_rx.try_recv() {
            if is_recording && !paused.load(Ordering::Relaxed) {
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

        // Show paused state changes
        // (SIGUSR1 toggles `paused`; we don't need to poll it explicitly except to ignore hotkey)

        match hotkey_manager.wait_for_action() {
            Ok(Some(HotkeyAction::Pressed)) if !is_recording => {
                if paused.load(Ordering::Relaxed) {
                    println!("⏸ paused — ignoring hotkey (toggle to resume)");
                    continue;
                }
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

#[allow(clippy::too_many_arguments)]
async fn run_dashboard_loop(
    cfg: &config::Config,
    stt_engine: &stt::WhisperStt,
    vad_engine: &mut SileroVad,
    hotkey_manager: &HotkeyManager,
    audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    terminal: &mut tui::TuiTerminal,
) -> Result<()> {
    use tui::{ui, WaveformHistory};

    let mut app = tui::build_initial_state(
        hotkey_manager.combo(),
        &cfg.whisper.model,
        cfg.vad.threshold,
    );
    app.mode = tui::AppMode::Dashboard;
    app.strategy = cfg.insertion.default.clone();

    let mut waveform = WaveformHistory::new(120);
    let mut is_recording = false;
    let mut audio_buffer: Vec<f32> = Vec::new();
    let mut tick: usize = 0;
    let mut last_app_check = Instant::now();
    let mut pending_vad_stop = false;

    terminal.draw(|f| ui::render_dashboard(f, &app, &waveform))?;

    while running.load(Ordering::Relaxed) {
        tick = tick.wrapping_add(1);
        app.tick = tick;
        app.paused = paused.load(Ordering::Relaxed);

        if let Ok(size) = terminal.size() {
            let cap = (size.width as usize).saturating_sub(10).clamp(60, 220);
            waveform.set_capacity(cap);
        }

        if last_app_check.elapsed() > Duration::from_millis(900) {
            last_app_check = Instant::now();
            let bundle = insert::app_detect::get_frontmost_bundle_id();
            app.app_name = bundle.clone();
            app.strategy = resolve_strategy_name(&bundle, cfg);
        }

        let mut had_chunk = false;
        let mut vad_triggered = false;
        while let Ok(chunk) = audio_rx.try_recv() {
            had_chunk = true;
            waveform.push_chunk(&chunk);
            if is_recording && !app.paused {
                audio_buffer.extend_from_slice(&chunk);
                app.recording_samples = audio_buffer.len();
                match vad_engine.process(&chunk) {
                    Ok(VadEvent::SpeechEnd) => {
                        tracing::info!("VAD: speech ended, auto-stopping");
                        vad_triggered = true;
                        app.vad_status = "speech end → auto-stop".to_string();
                    }
                    Ok(VadEvent::SpeechStart) => {
                        app.vad_status = "speech detected".to_string();
                    }
                    Ok(VadEvent::None) => {}
                    Err(e) => {
                        tracing::warn!("VAD error: {}", e);
                        app.vad_status = format!("vad err: {}", e);
                    }
                }
            }
        }

        if !had_chunk
            && tick % 3 == 0 {
                if is_recording && !app.paused {
                    waveform.push_level(0);
                } else if tick % 6 == 0 {
                    waveform.push_silence();
                }
            }

        if vad_triggered && is_recording {
            pending_vad_stop = true;
        }

        if pending_vad_stop && is_recording {
            pending_vad_stop = false;
            is_recording = false;
            app.is_recording = false;
            app.status = "VAD auto-stop — transcribing…".to_string();
            while let Ok(chunk) = audio_rx.try_recv() {
                audio_buffer.extend_from_slice(&chunk);
                waveform.push_chunk(&chunk);
            }
            app.recording_samples = audio_buffer.len();
            terminal.draw(|f| ui::render_dashboard(f, &app, &waveform))?;
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

        match hotkey_manager.try_action() {
            Ok(Some(HotkeyAction::Pressed)) if !is_recording => {
                if app.paused {
                    app.status = "⏸ paused — `miccli toggle` to resume".to_string();
                } else {
                    is_recording = true;
                    app.is_recording = true;
                    audio_buffer.clear();
                    app.recording_samples = 0;
                    app.status = "● recording… hold to talk".to_string();
                    app.error = None;
                    app.vad_status = "listening…".to_string();
                    waveform.clear();
                    tracing::info!("● recording started (dashboard)");
                }
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
                terminal.draw(|f| ui::render_dashboard(f, &app, &waveform))?;
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

        terminal.draw(|f| ui::render_dashboard(f, &app, &waveform))?;

        if is_recording {
            tokio::time::sleep(Duration::from_millis(12)).await;
        } else {
            tokio::time::sleep(Duration::from_millis(33)).await;
        }
    }

    Ok(())
}

/// Overlay (whisperflow): blank when idle, small top box only while recording.
async fn run_overlay_loop(
    cfg: &config::Config,
    stt_engine: &stt::WhisperStt,
    vad_engine: &mut SileroVad,
    hotkey_manager: &HotkeyManager,
    audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) -> Result<()> {
    use tui::{ui, WaveformHistory};

    let mut app = tui::build_initial_state(
        hotkey_manager.combo(),
        &cfg.whisper.model,
        cfg.vad.threshold,
    );
    app.mode = tui::AppMode::Overlay;
    app.strategy = cfg.insertion.default.clone();

    use crate::tui::waveform::waveform_to_blocks;

    let mut waveform = WaveformHistory::new(120);
    let mut is_recording = false;
    let mut audio_buffer: Vec<f32> = Vec::new();
    let mut tick: usize = 0;
    let mut last_app_check = Instant::now();
    // Native floating window (global, visible on opencode/other terminals) — preferred.
    // Falls back to terminal alt-screen overlay when not on macOS or window creation fails.
    let native_overlay = overlay::spawn();
    let has_native = native_overlay.is_some();
    let mut term_overlay: Option<tui::TuiTerminal> = None;
    let mut pending_vad_stop = false;
    let mut result_hold_until: Option<Instant> = None;

    tracing::info!(
        "miccli overlay running, hotkey {}, native_window={}, terminal blank when idle",
        hotkey_manager.combo(),
        has_native
    );

    // Helpers for showing/hiding overlay (native or terminal fallback)
    let show_overlay = |app: &tui::AppState, waveform: &WaveformHistory, native: &Option<overlay::OverlayHandle>, term: &mut Option<tui::TuiTerminal>| {
        if let Some(h) = native {
            h.show();
            h.set_recording(app.is_recording);
            h.set_paused(app.paused);
            let blocks = waveform_to_blocks(&waveform.data(), 48);
            h.waveform(blocks);
            let txt = if app.is_recording { "listening…".to_string() } else { app.transcription.clone() };
            h.transcription(txt);
            h.status(app.status.clone());
        } else {
            if term.is_none() {
                if let Ok(t) = tui::init_overlay_terminal() {
                    *term = Some(t);
                }
            }
            if let Some(t) = term.as_mut() {
                let _ = t.draw(|f| ui::render_overlay(f, app, waveform));
            }
        }
    };
    let hide_overlay = |native: &Option<overlay::OverlayHandle>, term: &mut Option<tui::TuiTerminal>| {
        if let Some(h) = native {
            h.hide();
        }
        if let Some(mut t) = term.take() {
            let _ = tui::restore_terminal(&mut t);
        }
    };

    while running.load(Ordering::Relaxed) {
        tick = tick.wrapping_add(1);
        app.tick = tick;
        app.paused = paused.load(Ordering::Relaxed);

        // App detection throttled
        if last_app_check.elapsed() > Duration::from_millis(900) {
            last_app_check = Instant::now();
            let bundle = insert::app_detect::get_frontmost_bundle_id();
            app.app_name = bundle.clone();
            app.strategy = resolve_strategy_name(&bundle, cfg);
        }

        // Drain audio
        let mut had_chunk = false;
        let mut vad_triggered = false;
        while let Ok(chunk) = audio_rx.try_recv() {
            had_chunk = true;
            if is_recording && !app.paused {
                waveform.push_chunk(&chunk);
                if has_native {
                    if let Some(h) = native_overlay.as_ref() {
                        let blocks = waveform_to_blocks(&waveform.data(), 48);
                        h.waveform(blocks);
                    }
                }
                audio_buffer.extend_from_slice(&chunk);
                app.recording_samples = audio_buffer.len();
                match vad_engine.process(&chunk) {
                    Ok(VadEvent::SpeechEnd) => {
                        tracing::info!("VAD: speech ended, auto-stopping");
                        vad_triggered = true;
                        app.vad_status = "speech end → auto-stop".to_string();
                    }
                    Ok(VadEvent::SpeechStart) => {
                        app.vad_status = "speech detected".to_string();
                    }
                    Ok(VadEvent::None) => {}
                    Err(e) => {
                        tracing::warn!("VAD error: {}", e);
                        app.vad_status = format!("vad err: {}", e);
                    }
                }
            }
        }

        if !had_chunk && is_recording && !app.paused
            && tick % 3 == 0 {
                waveform.push_level(0);
                if has_native {
                    if let Some(h) = native_overlay.as_ref() {
                        let blocks = waveform_to_blocks(&waveform.data(), 48);
                        h.waveform(blocks);
                    }
                }
            }

        if vad_triggered && is_recording {
            pending_vad_stop = true;
        }

        // Adapt width for terminal fallback (native has fixed 48)
        if !has_native {
            if let Some(term) = term_overlay.as_mut() {
                if let Ok(size) = term.size() {
                    let cap = (size.width as usize).saturating_sub(10).clamp(40, 180);
                    waveform.set_capacity(cap);
                }
            }
        }

        // Handle VAD auto-stop while overlay is up
        if pending_vad_stop && is_recording {
            pending_vad_stop = false;
            is_recording = false;
            app.is_recording = false;
            // Drain remaining
            while let Ok(chunk) = audio_rx.try_recv() {
                audio_buffer.extend_from_slice(&chunk);
                waveform.push_chunk(&chunk);
            }
            app.recording_samples = audio_buffer.len();
            app.status = "VAD auto-stop — transcribing…".to_string();
            if has_native {
                if let Some(h) = native_overlay.as_ref() {
                    h.set_recording(false);
                    h.status(app.status.clone());
                }
            } else if let Some(term) = term_overlay.as_mut() {
                let _ = term.draw(|f| ui::render_overlay(f, &app, &waveform));
            }
            let buf = std::mem::take(&mut audio_buffer);
            match process_and_insert_tui(&buf, stt_engine, vad_engine, &cfg.llm, &cfg.insertion).await {
                Ok(res) => {
                    if let Some(r) = res {
                        app.transcription = r.text.clone();
                        app.raw_text = r.raw.clone();
                        app.latencies = Some(r.latencies);
                        app.status = r.status.clone();
                        app.error = r.error.clone();
                        app.vad_status = r.vad_info.clone();
                        if has_native {
                            if let Some(h) = native_overlay.as_ref() {
                                h.transcription(r.text.clone());
                                h.status(r.status.clone());
                            }
                        }
                        if let Some(bundle) = r.bundle_id {
                            app.app_name = Some(bundle.clone());
                            app.strategy = resolve_strategy_name(&Some(bundle), cfg);
                        }
                    } else {
                        app.status = "No speech detected — try again".to_string();
                        app.vad_status = "no speech".to_string();
                        app.transcription.clear();
                        if has_native {
                            if let Some(h) = native_overlay.as_ref() {
                                h.status(app.status.clone());
                            }
                        }
                    }
                }
                Err(e) => {
                    app.error = Some(e.to_string());
                    app.status = "Transcription failed".to_string();
                    if has_native {
                        if let Some(h) = native_overlay.as_ref() {
                            h.status(app.status.clone());
                        }
                    }
                }
            }
            if has_native {
                if let Some(h) = native_overlay.as_ref() {
                    h.transcription(app.transcription.clone());
                }
            } else if let Some(term) = term_overlay.as_mut() {
                let _ = term.draw(|f| ui::render_overlay(f, &app, &waveform));
            }
            result_hold_until = Some(Instant::now() + Duration::from_millis(1400));
            // Don't hide immediately — let result_hold logic below handle it
            waveform.push_level(0);
        }

        // Hotkey handling (non-blocking)
        match hotkey_manager.try_action() {
            Ok(Some(HotkeyAction::Pressed)) if !is_recording => {
                if app.paused {
                    // Briefly show paused overlay (native or terminal)
                    if has_native {
                        if let Some(h) = native_overlay.as_ref() {
                            h.show();
                            h.set_paused(true);
                            h.set_recording(false);
                            h.status("⏸ paused — `miccli toggle` to resume".to_string());
                        }
                    } else {
                        show_overlay(&app, &waveform, &native_overlay, &mut term_overlay);
                    }
                    app.status = "⏸ paused — `miccli toggle` to resume".to_string();
                    result_hold_until = Some(Instant::now() + Duration::from_millis(1500));
                } else {
                    is_recording = true;
                    app.is_recording = true;
                    audio_buffer.clear();
                    app.recording_samples = 0;
                    app.status = "● recording… hold to talk".to_string();
                    app.error = None;
                    app.vad_status = "listening…".to_string();
                    waveform.clear();
                    result_hold_until = None;
                    if has_native {
                        if let Some(h) = native_overlay.as_ref() {
                            h.show();
                            h.set_paused(false);
                            h.set_recording(true);
                            h.waveform(String::new());
                            h.transcription("listening…".to_string());
                            h.status(app.status.clone());
                        }
                    } else {
                        show_overlay(&app, &waveform, &native_overlay, &mut term_overlay);
                    }
                    tracing::info!("● recording started (overlay)");
                }
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
                if has_native {
                    if let Some(h) = native_overlay.as_ref() {
                        h.set_recording(false);
                        h.status(app.status.clone());
                    }
                } else if let Some(term) = term_overlay.as_mut() {
                    let _ = term.draw(|f| ui::render_overlay(f, &app, &waveform));
                }
                let buf = std::mem::take(&mut audio_buffer);
                match process_and_insert_tui(&buf, stt_engine, vad_engine, &cfg.llm, &cfg.insertion).await {
                    Ok(res) => {
                        if let Some(r) = res {
                            app.transcription = r.text.clone();
                            app.raw_text = r.raw.clone();
                            app.latencies = Some(r.latencies);
                            app.status = r.status.clone();
                            app.error = r.error.clone();
                            app.vad_status = r.vad_info.clone();
                            if has_native {
                                if let Some(h) = native_overlay.as_ref() {
                                    h.transcription(r.text.clone());
                                    h.status(r.status.clone());
                                }
                            }
                            if let Some(bundle) = r.bundle_id {
                                app.app_name = Some(bundle.clone());
                                app.strategy = resolve_strategy_name(&Some(bundle), cfg);
                            }
                        } else {
                            app.status = "No speech detected — try again".to_string();
                            app.vad_status = "no speech".to_string();
                            app.transcription.clear();
                            if has_native {
                                if let Some(h) = native_overlay.as_ref() {
                                    h.status(app.status.clone());
                                }
                            }
                        }
                    }
                    Err(e) => {
                        app.error = Some(e.to_string());
                        app.status = "Transcription failed".to_string();
                        if has_native {
                            if let Some(h) = native_overlay.as_ref() {
                                h.status(app.status.clone());
                            }
                        }
                    }
                }
                if has_native {
                    if let Some(h) = native_overlay.as_ref() {
                        h.transcription(app.transcription.clone());
                    }
                } else if let Some(term) = term_overlay.as_mut() {
                    let _ = term.draw(|f| ui::render_overlay(f, &app, &waveform));
                }
                result_hold_until = Some(Instant::now() + Duration::from_millis(1400));
            }
            Ok(Some(HotkeyAction::Released)) => {}
            Ok(None) => {}
            Err(()) => break,
        }

        // Handle overlay visibility and q-to-quit
        if has_native {
            // Native floating window is globally visible (opencode/other terminals).
            // Terminal where daemon runs stays blank; we still poll it for q to quit daemon.
            if let Ok(action) = tui::poll_key_action(Duration::from_millis(0)) {
                if matches!(action, tui::TuiKeyAction::Quit) {
                    running.store(false, Ordering::Relaxed);
                    break;
                }
            }
            if !is_recording {
                if let Some(until) = result_hold_until {
                    if Instant::now() >= until {
                        hide_overlay(&native_overlay, &mut term_overlay);
                        result_hold_until = None;
                    }
                }
            }
        } else if let Some(term) = term_overlay.as_mut() {
            // Poll for q/Ctrl+C to quit daemon while overlay is up
            if let Ok(action) = tui::poll_key_action(Duration::from_millis(0)) {
                if matches!(action, tui::TuiKeyAction::Quit) {
                    running.store(false, Ordering::Relaxed);
                    break;
                }
            }
            // Draw overlay (top bar)
            let _ = term.draw(|f| ui::render_overlay(f, &app, &waveform));

            // Check if we should auto-hide after result display
            if !is_recording {
                if let Some(until) = result_hold_until {
                    if Instant::now() >= until {
                        hide_overlay(&native_overlay, &mut term_overlay);
                        result_hold_until = None;
                    }
                }
            }
        } else {
            // No overlay — check for hold timeout to hide if we had a result (native case already handled)
            if let Some(until) = result_hold_until {
                if Instant::now() >= until {
                    result_hold_until = None;
                }
            }
            // Also poll for q to quit daemon even when idle (blank terminal)
            if let Ok(action) = tui::poll_key_action(Duration::from_millis(0)) {
                if matches!(action, tui::TuiKeyAction::Quit) {
                    running.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }

        if is_recording {
            tokio::time::sleep(Duration::from_millis(12)).await;
        } else if term_overlay.is_some() || has_native && result_hold_until.is_some() {
            tokio::time::sleep(Duration::from_millis(33)).await;
        } else {
            // Idle, no overlay — very low CPU
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    // Ensure overlay is torn down if still up
    hide_overlay(&native_overlay, &mut term_overlay);

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

pub fn status() -> Result<()> {
    let pid_file = pid_file_path()?;
    let log_path = default_log_path()?;
    if !pid_file.exists() {
        println!("miccli is not running (no PID file)");
        if log_path.exists() {
            println!("log: {}", log_path.display());
            if let Ok(meta) = fs::metadata(&log_path) {
                println!("log size: {} bytes", meta.len());
            }
        }
        return Ok(());
    }
    let pid_str = fs::read_to_string(&pid_file)?;
    let pid: i32 = match pid_str.trim().parse() {
        Ok(v) => v,
        Err(_) => {
            println!("miccli PID file is invalid, removing stale file");
            let _ = fs::remove_file(&pid_file);
            return Ok(());
        }
    };
    if is_process_alive(pid) {
        println!("miccli is running (PID {})", pid);
        println!("PID file: {}", pid_file.display());
        println!("log: {} {}", log_path.display(), if log_path.exists() { "" } else { "(not yet created)" });
        if log_path.exists() {
            if let Ok(meta) = fs::metadata(&log_path) {
                println!("log size: {} bytes", meta.len());
            }
            // Show last few lines if small
            if let Ok(content) = fs::read_to_string(&log_path) {
                let lines: Vec<&str> = content.lines().collect();
                if !lines.is_empty() {
                    println!("--- last log lines ---");
                    for line in lines.iter().rev().take(5).rev() {
                        println!("{}", line);
                    }
                }
            }
        }
        // Try to detect if process is backgrounded (PPID 1)
        #[cfg(unix)]
        {
            // Check via ps or just report
            println!("use `miccli stop` to stop, `miccli toggle` to pause/resume, `miccli restart` to restart");
        }
    } else {
        println!("miccli PID file is stale (PID {} not running), removing", pid);
        let _ = fs::remove_file(&pid_file);
        println!("miccli is not running");
        if log_path.exists() {
            println!("log: {}", log_path.display());
        }
    }
    Ok(())
}

pub fn send_signal(signal: &str) -> Result<()> {
    let pid_file = pid_file_path()?;

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
