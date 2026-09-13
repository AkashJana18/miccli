# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-09-13

### Added
- **Ratatui TUI (default)** — `miccli start` now shows a live TUI by default (fallback to plain logs with `--no-tui` or when not a TTY). Includes:
  - Live tab: waveform sparkline, transcription preview (raw vs cleaned), pipeline latencies (transcribe/cleanup/insert/total), and insertion target (app name + `type`/`paste` strategy)
  - Real-time **waveform visualization** (`src/tui/waveform.rs`) — RMS + peak blended to `0..100` levels with silence gating/decay, responsive to terminal width (60–220 cols), color-coded by amplitude
  - **Models tab**: table of Whisper models (`tiny` 75 MB, `base` 142 MB, `small` 466 MB, `medium` 1.5 GB) with `✅ downloaded`/`○ not downloaded` status and active-model marker (`▶`)
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

[Unreleased]: https://github.com/AkashJana18/miccli/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/AkashJana18/miccli/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/AkashJana18/miccli/releases/tag/v0.1.0
