# miccli

Terminal voice dictation CLI — local Whisper STT + code-aware cleanup + smart text insertion.

Hold a hotkey, speak, release. Your voice becomes text, instantly in your terminal.

**Why miccli exists:** Claude Code, Codex, and opencode collapse pasted multi-line text into `[Pasted text]`. miccli detects this and types character-by-character instead, so everything arrives intact.

## Features

- **Local Whisper STT** — no API key needed, runs offline via whisper-rs + Metal acceleration
- **App-aware text insertion** — detects frontmost app, routes to slow char typing (TUIs) or fast clipboard paste (editors)
- **Two-tier cleanup** — regex rules for 56+ symbols (instant), optional LLM polish via Ollama/Groq
- **Global hotkey** — Cmd+Fn hold-to-talk (configurable)
- **Model management** — download, list, remove whisper models from CLI

## Install

```bash
cargo install miccli
```

Or build from source:

```bash
git clone https://github.com/miccli/miccli.git
cd miccli
cargo build --release
# binary at target/release/miccli
```

## Quick Start

```bash
# Download a whisper model (first time only)
miccli models download small

# Start listening
miccli start
```

Hold **Cmd+Fn** (default hotkey), speak, release. Text appears in your active app.

## Commands

| Command | Description |
|---------|-------------|
| `miccli start` | Start daemon, listen for hotkey |
| `miccli stop` | Stop daemon |
| `miccli toggle` | Toggle recording on/off |
| `miccli config` | Show config path, open in editor |
| `miccli models` | List/download/remove whisper models |
| `miccli models download <size>` | Download a model (tiny/base/small/medium) |
| `miccli models list` | List installed models |

## Config

Config lives at `~/.config/miccli/config.toml`:

```toml
[hotkey]
key = "Fn"
modifier = "Command"          # Command | Option | Control | Shift

[whisper]
model = "small"               # tiny | base | small | medium
language = "en"               # ISO 639-1 code
metal = true                  # Apple Metal acceleration

[vad]
threshold = 0.5               # Voice activity detection threshold

[llm]
provider = "ollama"           # ollama | groq | openai | none
model = "qwen2.5:1.5b"       # Model name
enabled = false               # Enable LLM cleanup

[insertion]
default = "auto"              # auto | type | clipboard
key_delay_ms = 20             # Delay between keystrokes (type mode)
restore_clipboard = true      # Restore clipboard after paste
```

### App overrides

Override insertion strategy per app:

```toml
[[insertion.apps]]
bundle_id = "com.anthropic.claudefordesktop"
strategy = "type"             # type | clipboard | paste
```

## Terminal Insertion

miccli solves the `[Pasted text]` collapse problem in terminal TUIs:

| App | Strategy | Why |
|-----|----------|-----|
| Terminal.app, iTerm2, Alacritty, Kitty, Ghostty | **Type** (20ms/char) | Paste triggers bracket collapse |
| Claude Code, opencode, Codex | **Type** (20ms/char) | Electron TUIs have same issue |
| VS Code, IntelliJ, Sublime | **Paste** (clipboard) | Full paste support |

The `auto` mode (default) detects the frontmost app via `osascript` and picks the right strategy. You can override per-app in config, or force a global strategy.

## LLM Cleanup

Optional code-aware cleanup via local or cloud LLMs:

**Ollama (free, local):**
```bash
ollama pull qwen2.5:1.5b
```
Then in config: `provider = "ollama"`, `enabled = true`

**Groq (free tier):**
```bash
export GROQ_API_KEY=gsk_...
```
Then in config: `provider = "groq"`, `enabled = true`

**How it works:**
1. Regex rules run first (instant, handles ~60% of cases)
2. LLM runs only if text > 20 chars and not just a symbol (fast path)
3. LLM receives user config + dictation context for code-aware results

## Building from Source

Requirements:
- Rust 1.75+
- macOS (Linux support planned)

```bash
cargo build --release
```

## License

MIT
