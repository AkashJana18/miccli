# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-08-27

### Added
- Local Whisper STT via whisper-rs with Metal acceleration
- App-aware text insertion: slow char typing for TUIs, fast clipboard paste for editors
- Solves `[Pasted text]` collapse in Claude Code, opencode, Codex
- Two-tier text cleanup: regex rules for 56+ spoken symbols + LLM (Ollama/Groq/OpenAI)
- Global hotkey via Carbon (Cmd+Fn default, hold-to-talk)
- Whisper model management: download, list, remove from CLI
- Config at `~/.config/miccli/config.toml`
- Energy-based VAD placeholder (Silero integration planned)
- MCP server subcommand (planned)

[0.1.0]: https://github.com/miccli/miccli/releases/tag/v0.1.0
