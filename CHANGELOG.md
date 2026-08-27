# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Silero VAD v5 (via ort ONNX) replacing the energy-based placeholder; auto-downloads model to `~/.config/miccli/silero_vad.onnx`, auto-stops dictation on speech end

### Changed
- Bundled ONNX Runtime statically via `download-binaries` (fixes `dlopen` failure of the ONNX Runtime dylib at launch)
- Hotkey rewritten using a CoreGraphics event tap: supports **modifier-only hold-to-talk** (default `Shift+Control`), which the previous Carbon-based hotkey could not (the `Fn` key, and bare modifier combos, cannot be registered as Carbon global hotkeys). Requires Accessibility permission.
- Hold-to-talk semantics: press-and-hold to record, release to transcribe (previously toggle)

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

[0.1.0]: https://github.com/miccli/miccli/releases/tag/v0.1.0
