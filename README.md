# 🎙️ Miccli

[![CI](https://github.com/AkashJana18/miccli/actions/workflows/ci.yml/badge.svg)](https://github.com/AkashJana18/miccli/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/tag/AkashJana18/miccli?label=version&sort=semver)](https://github.com/AkashJana18/miccli/releases)
[![Crates.io](https://img.shields.io/crates/v/miccli)](https://crates.io/crates/miccli)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS-lightgrey.svg)](#requirements)

Terminal voice dictation for macOS. Hold a hotkey, speak, release then transcribed text is inserted into the focused application. Runs fully offline with local Whisper.

## Features

- **Local transcription** — Whisper via `whisper-rs` with Metal acceleration, no API key required.
- **Overlay** — Minimal top bar with live waveform and transcription, visible only while recording; terminal is blank when idle.
- **Dashboard** — Four-tab TUI (Live, Models, Config, Help) for monitoring and management.
- **App-aware insertion** — Automatically uses typed input for terminals and TUIs, clipboard paste for editors.
- **Two-tier cleanup** — Instant regex rules for spoken symbols plus optional LLM polishing (Ollama, Groq, OpenAI).
- **Global hotkey** — Modifier-only hold-to-talk (`Shift+Control` default) via `CGEventTap`.
- **Daemon** — Foreground or background mode with PID management, logging, pause/resume, and status.

## Requirements

- macOS 13+ (Apple Silicon recommended)
- Rust 1.80+ (to build from source)
- Microphone and Accessibility permissions (System Settings → Privacy & Security)

Accessibility permission is required for the global hotkey and text insertion. Without it, the hotkey will not fire.

## Installation

From crates.io:

```bash
cargo install miccli
```

From source:

```bash
git clone https://github.com/AkashJana18/miccli.git
cd miccli
cargo build --release
# binary at target/release/miccli (24 MB release)
```

## Quick Start

```bash
# Download a Whisper model (first run)
miccli models download small

# Start dictation (overlay hidden until hotkey is held)
miccli start
# Hold Shift+Control, speak, release

# Run in background
miccli start --background
# Logs: ~/.config/miccli/miccli.log (override with --log-file)

# Other commands
miccli status        # PID, log, stale-PID detection
miccli dashboard     # Full TUI — q to quit, daemon restarts
miccli toggle        # Pause / resume
miccli restart --background
miccli stop
```

Run `miccli start` in a separate pane from the application you dictate into. The hotkey is global, so it works even when the overlay terminal is not focused.

First run will prompt for:

1. **LLM cleanup** — Disabled by default. Options: disabled, Ollama (local), Groq (BYOK), OpenAI (BYOK). Saved to `~/.config/miccli/config.toml`. Set `MICCLI_NO_PROMPT=1` to skip.
2. **Overlay** — Enabled (floating HUD) or disabled (plain logs). Still records and inserts in both modes. Change later via `tui.mode`.

## Configuration

Config file: `~/.config/miccli/config.toml`

```toml
[hotkey]
key = ""                      # empty for modifier-only hold
modifier = "Shift+Control"    # Command | Option | Control | Shift, joined with "+"

[whisper]
model = "small"               # tiny | base | small | medium
language = "en"
metal = true

[vad]
threshold = 0.5
min_speech_ms = 250
min_silence_ms = 500

[llm]
provider = "ollama"           # ollama | groq | openai | none
model = "qwen2.5:1.5b"
enabled = false

[insertion]
default = "auto"              # auto | type | clipboard
key_delay_ms = 20
paste_delay_ms = 10
restore_clipboard = true

[tui]
mode = "overlay"              # overlay | dashboard | none
```

Edit the file and run `miccli restart [--background]` to apply changes.

### App overrides

```toml
[[insertion.apps]]
bundle_id = "com.anthropic.claudefordesktop"
strategy = "type"
```

### TUI modes

- `overlay` — Minimal top bar only while holding the hotkey. Auto-hides after release.
- `dashboard` — Persistent 4-tab interface. Use `miccli dashboard` or set `tui.mode = "dashboard"`.
- `none` — No visual overlay, plain log output. Dictation and insertion still active. Falls back automatically when stdout is not a TTY.

## Commands

| Command | Description |
|---------|-------------|
| `miccli start` | Start daemon (blocks, blank when idle) |
| `miccli start --background` | Daemonize, log to `~/.config/miccli/miccli.log` |
| `miccli status` | Show PID, log size/tail, stale-PID check |
| `miccli restart [--background]` | Stop then start |
| `miccli dashboard` | Stop daemon, open TUI, restart on exit |
| `miccli toggle` | Pause / resume |
| `miccli stop` | Stop daemon |
| `miccli config [--path]` | Show config location |
| `miccli models list` | List installed models |
| `miccli models download <size>` | Download `tiny` / `base` / `small` / `medium` |
| `miccli models remove <name>` | Remove a model |

## How it works

### Text insertion

`auto` mode detects the frontmost application via `osascript` and selects:

- **Type** (20ms/char) — Terminal.app, iTerm2, Alacritty, Kitty, Ghostty, WezTerm, Claude Code, opencode, Codex. Avoids `[Pasted text]` collapse in TUIs.
- **Paste** (clipboard) — VS Code, IntelliJ, Sublime Text and other editors.

All nested CLI tools inside a terminal inherit the `type` strategy, so dictation works for `opencode`, `nvim`, etc. Override with `[[insertion.apps]]` if needed. For VS Code's integrated terminal, set `bundle_id = "com.microsoft.VSCode"` to `type`.

### Cleanup

1. Regex rules handle 50+ spoken symbols (e.g., "open curly brace" → `{`) instantly.
2. If `llm.enabled = true` and text exceeds 20 characters, an LLM pass refines punctuation and removes filler words.

Ollama (local):

```bash
ollama pull qwen2.5:1.5b
```

Groq / OpenAI (BYOK):

```bash
export GROQ_API_KEY=gsk_...
export OPENAI_API_KEY=sk_...
```

## Development

```bash
cargo build --release
cargo test       # 56 passed, 3 ignored
cargo clippy
```

Release binary is ~24 MB (stripped, LTO). The daemon uses ~0.1% CPU when idle and ~0.5–1% while recording; memory ~24 MB.

## License

MIT — see [LICENSE](LICENSE).
