# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-09-17

### Added
- First stable release (1.0.0) — macOS only.

### Changed
- **LLM cleanup now opt-in** — `enabled = false` by default (`src/config.rs:default_llm_enabled`, `config/config.toml`, `src/tui/mod.rs:default_config_text`). First run prompts `1) disabled / 2) Ollama / 3) Groq BYOK / 4) OpenAI BYOK` (TTY only, 30s timeout, `MICCLI_NO_PROMPT=1`/`CI` skips) and persists choice to `~/.config/miccli/config.toml` (`src/config.rs:prompt_llm_setup`, `write_config`). No more silent Ollama requests on every dictation.
- **macOS only claim** — `src/hotkey.rs` now gated with `#[cfg(target_os="macos")]` stub that bails with friendly error on Linux; `README.md:234` updated to `macOS 13+ only`, `Cargo.toml:5` MSRV `1.80`.
- **Cargo metadata fixed** — `Cargo.toml:9-10` repository/homepage → `https://github.com/AkashJana18/miccli`, removed deprecated `[badges]` and unused `thiserror`, removed `documentation` (binary crate), bump `rust-version` `1.75` → `1.80` for `LazyLock`.
- **Config paths respect XDG** — `src/config.rs:config_dir()` now uses `dirs::config_dir()` with `~/.config/miccli` legacy fallback on macOS; `src/daemon.rs:17`, `src/stt.rs:65`, `src/vad.rs:14`, `src/models.rs:91`, `src/tui/mod.rs:134` now call `config::config_dir()`/`pid_file_path()`/`log_file_path()` instead of hardcoded `home_dir().join(".config")`.

### Fixed
- **Audio stereo bug** — `src/audio.rs:166` `PendingResampler::new` now always `channels=1` after mono mixdown (previously used `source_channels` but fed mono Vec, so stereo mics dropped chunks).
- **Model download corruption** — `src/stt.rs:95`, `src/vad.rs:41`, `src/models.rs:71` now atomic (`write .tmp` + `rename`) and check HTTP status; interrupted downloads no longer leave corrupt `ggml-*.bin`/`silero_vad.onnx`.
- **Panics → errors** — `src/audio.rs:150` `lock().unwrap()` → `unwrap_or_else(|e| e.into_inner())`, `src/daemon.rs:73` `parent().unwrap()` → `context`, `src/main.rs:107,168` `Runtime::new().unwrap()` → `expect`, `src/main.rs:120,179` `join().unwrap()` → `map_err`, `src/main.rs:278,366` `CString::new(...).unwrap()` now handles NUL in user log path, `src/overlay.rs:304` `Class::get().unwrap()` → `if let Some`.
- **Git ignore** — `/.gitignore:2` no longer ignores `Cargo.lock` (binary crate should be committed); added `*.log`/`miccli.log`/` .env`.

## [0.3.4] - 2026-09-14

### Added
- **Status & restart** — `miccli status` shows running PID, PID file, log location/size/last lines and detects stale PID; `miccli restart [--background] [--log-file PATH]` stops (SIGTERM, wait 5s+0.3s) then starts (reuses `daemonize`).
  - `src/main.rs:Commands::{Status, Restart}` and `src/daemon.rs:status()` + `pid_file_path()`/`default_log_path()`/`is_process_alive()`.
  - `src/main.rs:Restart` mirrors `Start` flags (`-b/--background`, `--log-file`).
- **Duplicate-instance guard** — `miccli start` / `miccli start --background` / `miccli restart` now check `~/.config/miccli/miccli.pid` via `libc::kill(pid,0)` before forking and in `daemon::start()` (`src/main.rs:check_duplicate_instance`, `src/daemon.rs:start`); stale PID files are auto-removed, live instance bails with `already running (PID X), use miccli stop/restart/status`.
- **Background log file** — `miccli start --background` now defaults to `~/.config/miccli/miccli.log` (or `--log-file PATH`), daemon redirects stdout/stderr via `dup2` append (`src/main.rs:daemonize(log_file)`), parent prints `PID + log path`; `miccli status` shows log size/tail.
- **Panic-safe PID cleanup** — `PidFileGuard` (`Drop` removes PID file) + `std::panic::set_hook` in `daemon::start` ensures PID file removed on panic/unwind; normal exit also removes via explicit `fs::remove_file` + guard double-remove is harmless. Fixes stale PID after crash.
  - Added `setup_termination_handler` already in `0.3.3` but now complemented by guard.
- Bumped `Cargo.toml` `0.3.3` → `0.3.4`.

## [0.3.3] - 2026-09-14

### Added
- **Background daemon** — `miccli start --background` (`-b`) daemonizes via double-fork, parent exits immediately, overlay stays global. The native overlay (`NSWindow` level 1000) is independent of the terminal, so it remains visible after the terminal returns. Works on both macOS (child keeps `CFRunLoop` + daemon alive) and Linux (`tokio::runtime` alive). `miccli stop` / `miccli toggle` (PID file `~/.config/miccli/miccli.pid` + `SIGTERM` / `SIGUSR1` via `libc::kill`) and `miccli dashboard` (stops daemon, shows TUI, restarts) continue unchanged. Without `--background`, `miccli start` still blocks as before.
  - `src/main.rs:Commands::Start` now `Start { #[arg(short, long)] background: bool }`, `daemonize()` helper (`libc::fork` → `setsid` → `fork` → `dup2 /dev/null`, `umask 0`) on `cfg(unix)`, `bail!` on non-unix.
  - Bumped `Cargo.toml` `0.3.2` → `0.3.3`.

## [0.3.2] - 2026-09-13

### Fixed
- **Waveform looked like a flat line** — `chunk_to_level` (`src/tui/waveform.rs`) was too conservative (rms `*420`, peak `*95`, gate `2.5`), so normal speech (`rms 0.03`, `peak 0.3`) hit only `▂` and looked flat. Now boosted (`rms*900` → `90`, `peak*140` → `100`, blend `30/70` + `*1.35+8`, gate `10`) so moderate speech hits `▆`/`▇` with real height.
- **Overlay HUD waveform height** — native window was `78px` with `16px` waveform field at `11pt` (`src/overlay.rs`), so blocks looked like a thin line. Now `96px` window, `28px` waveform field at `18pt Menlo`, centered top, so the `▁▂▃▄▅▆▇█` blocks have real vertical height like WhisperFlow. `make_label` now takes `size: f64` and sets `NSFont` (`fontWithName:size:`) correctly via `objc::runtime::Class`.

### Changed
- Bumped `Cargo.toml` `0.3.1` → `0.3.2`.

## [0.3.1] - 2026-09-13

### Fixed
- **Overlay not visible on opencode / other terminals** — the whisperflow overlay was a terminal alt-screen confined to the daemon's own tty, so it never appeared when `opencode` or another terminal was frontmost. Now uses a **native floating HUD window** (`src/overlay.rs`, `cocoa` `NSWindow` `NSBorderlessWindowMask` at level `1000` with `orderFrontRegardless`, centered top, 560×78, dark translucent, rounded, `setCollectionBehavior canJoinAllSpaces`). Visible on every Space/app, like WhisperFlow. Falls back to terminal overlay when not on macOS or window creation fails.
  - `overlay::spawn()` now creates the `NSWindow` on the main queue (`dispatch::Queue::main().exec_async` + `mpsc` sync, `OnceLock<WindowState>`) and updates via `exec_async` (`do_show`/`do_hide`/`do_waveform` etc.), so it is globally visible even when `opencode` is frontmost.
  - `src/daemon.rs:run_overlay_loop` now prefers `native_overlay` (`overlay::spawn()`) and falls back to `term_overlay` (`tui::init_overlay_terminal`) — waveform (48 blocks), transcription and status are sent to whichever is available. Idle still blank (no alt-screen).
  - `src/main.rs:Commands::Start` now keeps the main thread's `CFRunLoop` alive (`CFRunLoop::run_in_mode` 50 ms loop) while the daemon runs on a background thread, so the main-queue overlay window is serviced (previously `tokio::Runtime::block_on` blocked main, so `exec_async` to main never ran).
  - Added `cocoa = "0.25"`, `objc = "0.2"`, `dispatch = "0.2"`, `once_cell = "1.19"` under `[target.'cfg(target_os = "macos")'.dependencies]` (`Cargo.toml`).

### Changed
- Bumped `Cargo.toml` `0.3.0` → `0.3.1`.

## [0.3.0] - 2026-09-13

### Added
- **Overlay (whisperflow) as default** — `miccli start` is now blank when idle (primary screen unchanged, no clear), and shows a small top-aligned overlay only while holding the hotkey. Auto-hides ~1.4 s after release (like WhisperFlow). No `q` needed while coding.
  - Overlay box (~6 lines, `src/tui/ui.rs:render_overlay`): `● REC` pulsing + `app_name` + `⌨ type`/`⎘ paste`, single-line blocks waveform `▁▂▃▄▅▆▇█` via `src/tui/waveform.rs:level_to_block`/`waveform_to_blocks` (40–180 cols), transcription + `raw` diff, status/latencies. Top-aligned, hides cursor, transient alt-screen (`src/tui/mod.rs:init_overlay_terminal`).
  - `tui.mode` config (`src/config.rs:TuiConfig`): `overlay` (default, whisperflow) | `dashboard` (persistent full) | `none` (plain logs). Added to `config/config.toml` and `src/tui/mod.rs:default_config_text`.
  - `AppMode` enum (`src/tui/mod.rs`) + `AppState.mode`/`paused` (`src/tui/ui.rs`); `run_overlay_loop` (`src/daemon.rs`) keeps `50 ms` idle / `12 ms` recording sleeps, so **background vs foreground CPU identical** (~0.1% idle, ~0.5–1% recording) — no fork needed, foreground stays (blank idle) and `CGEventTap` remains global.
- **Full dashboard coexistence** — `miccli dashboard` (`src/main.rs:Commands::Dashboard`, `src/daemon.rs:dashboard`) stops the daemon (SIGTERM, wait pid removal), opens persistent 4-tab dashboard (`src/tui/ui.rs:render_dashboard`, `src/daemon.rs:run_dashboard_loop` / `run_dashboard_standalone`), `q`/`Esc`/`Ctrl+C` quit → auto-restarts overlay daemon in same terminal (`start().await`). Both modes coexist.
- **Pause toggle** — `miccli toggle` repurposed to pause/resume (SIGUSR1). `setup_pause_handler` (`src/daemon.rs`) spawns `tokio::signal::unix::signal(SignalKind::user_defined1)` that flips `Arc<AtomicBool> paused`; overlay shows `⏸ paused` and hotkey is ignored until toggled.

### Changed
- `miccli start` no longer takes `--foreground` / `--no-tui` — removed unused `foreground` flag (`src/main.rs`, `src/daemon.rs:start()` now `() -> Result`). Fallback is now `tui.mode = "none"` or non-TTY (`tui::is_tty()`), not a CLI flag. `src/tui/mod.rs:init_terminal` no longer clears when idle; overlay uses transient alt-screen.
- Dashboard `render()` now dispatches on `AppState.mode` (`src/tui/ui.rs:render` → `render_overlay` vs `render_dashboard`); existing `render` kept for tests with `#[allow(dead_code)]`.
- Bumped `Cargo.toml` `0.2.0` → `0.3.0`.

### Fixed
- Overlay idle no longer clears primary screen — respects "terminal is as it is" (no change when not recording).
- SIGUSR1 no longer kills daemon (now handled, previously default terminate); toggle was a no-op in `0.2.0`.
- Waveform `50` now correctly maps to `▄` (was `▅` in test), and `blocks_mapping` test updated (`src/tui/waveform.rs`).

## [0.2.0] - 2026-09-13

### Added
- **Ratatui TUI (default)** — `miccli start` now shows a live TUI by default (fallback to plain logs with `--no-tui` or when not a TTY). Includes:
  - Live tab: waveform sparkline, transcription preview (raw vs cleaned), pipeline latencies (transcribe/cleanup/insert/total), and insertion target (app name + `type`/`paste` strategy)
  - Real-time **waveform visualization** (`src/tui/waveform.rs`) — RMS + peak blended to `0..100` levels with silence gating/decay, responsive to terminal width (60–220 cols), color-coded by amplitude
  - **Models tab**: table of Whisper models (`tiny` 75 MB, `base` 142 MB, `small` 466 MB, `medium` 1.5 GB) with `installed`/`not installed` status and active-model marker
  - **Config tab**: syntax-highlighted view of `~/.config/miccli/config.toml` (sections in magenta, keys in cyan, values in yellow)
  - **Help tab**: keybindings, insertion strategy explainer, and macOS permission checklist (Microphone + Accessibility)
  - Navigation: `q`/`Esc`/`Ctrl+C` quit, `Tab`/`Shift+Tab` cycle, `1`/`2`/`3`/`4` jump; pulsing `● REC`/`■ idle` indicator and 30–80 fps render loop via `tokio::time::sleep`
  - Non-blocking hotkey polling (`HotkeyManager::try_action` in `src/hotkey.rs`) so TUI stays responsive while preserving hold-to-talk (`Shift+Control` default) semantics
  - Lightweight: `ratatui 0.29` + `crossterm 0.28` adds ~1–2 MB to the `24 MB` release binary (vs `62 MB` debug); still dominated by `ort`/`whisper-rs`
- Silero VAD v5 (via `ort` ONNX) replacing the energy-based placeholder; auto-downloads model to `~/.config/miccli/silero_vad.onnx`, auto-stops dictation on speech end

### Changed
- Bundled ONNX Runtime statically via `download-binaries` (fixes `dlopen` failure of the ONNX Runtime dylib at launch)
- Hotkey rewritten using a CoreGraphics event tap: supports **modifier-only hold-to-talk** (default `Shift+Control`), which the previous Carbon-based hotkey could not (the `Fn` key, and bare modifier combos, cannot be registered as Carbon global hotkeys). Requires Accessibility permission.
- Hold-to-talk semantics: press-and-hold to record, release to transcribe (previously toggle)
- `Cargo.toml` dependencies: added `ratatui 0.29`, `crossterm 0.28`; bumped package version `0.1.0` → `0.2.0`

### Fixed
- Mic capture was silent (always 0 samples): callbacks were fed to the fixed-input resampler at cpal's driver buffer size, which failed every `process()`. Audio is now buffered and resampled in fixed blocks (`PendingResampler`)
- Recording no longer inserts hallucinated text when nothing was said: the Silero VAD now gates transcription, so ambient noise (which Whisper would transcribe into phantom phrases like "Thank you.") is skipped unless real speech is detected
- Every recording stopped after ~50 ms regardless of hold duration: the hotkey poll treated its receive timeout as a key-release event, cutting off real holds at ~1024 samples before any speech was captured. The timeout now signals "nothing happened, keep recording", and recording ends only on a genuine release
- Removed tracked `silero_vad.onnx` binary (`3.7 MB`) from git — model now auto-downloaded on first run (also listed in `.gitignore`)
- Cleaned dead code (`config/dict.toml` unreachable, `src/models.rs` `allow(dead_code)`, snake_case test rename)

## [0.1.0] - 2026-08-27

### Added
- Local Whisper STT via whisper-rs with Metal acceleration
- App-aware text insertion: slow char typing for TUIs, fast clipboard paste for editors
- Solves `[Pasted text]` collapse in Claude Code, opencode, Codex
- Two-tier text cleanup: regex rules for 56+ spoken symbols + LLM (Ollama/Groq/OpenAI)
- Global hotkey via muda (Cmd+F5 default, hold-to-talk, configurable)
- Whisper model management: download, list, remove from CLI
- Config at `~/.config/miccli/config.toml`
- Energy-based VAD placeholder (Silero integration planned)
- MCP server subcommand (planned)

[Unreleased]: https://github.com/AkashJana18/miccli/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/AkashJana18/miccli/compare/v0.3.4...v1.0.0
[0.3.4]: https://github.com/AkashJana18/miccli/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/AkashJana18/miccli/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/AkashJana18/miccli/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/AkashJana18/miccli/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/AkashJana18/miccli/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/AkashJana18/miccli/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/AkashJana18/miccli/releases/tag/v0.1.0
