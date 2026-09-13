# miccli

Terminal voice dictation CLI — local Whisper STT + code-aware cleanup + smart text insertion.

Hold a hotkey, speak, release. Your voice becomes text, instantly in your terminal. Now with a live **ratatui TUI** — waveform, transcription preview, and model management in your terminal.

**Why miccli exists:** Claude Code, Codex, and opencode collapse pasted multi-line text into `[Pasted text]`. miccli detects this and types character-by-character instead, so everything arrives intact. It works in every nested terminal CLI (opencode, nvim, etc.) because terminal emulators are classified as `type` (see below).

## Features

- **Live TUI (ratatui)** — `miccli start` shows a dashboard by default: waveform sparkline, live transcription, latencies, and insertion target; tabs for Models/Config/Help; `q`/`Tab`/`1-4` navigation; `--no-tui` falls back to plain logs (still <2 MB overhead, 24 MB release)
- **Local Whisper STT** — no API key needed, runs offline via whisper-rs + Metal acceleration
- **App-aware text insertion** — detects frontmost app, routes to slow char typing (TUIs) or fast clipboard paste (editors)
- **Two-tier cleanup** — regex rules for 56+ symbols (instant), optional LLM polish via Ollama/Groq
- **Global hotkey** — modifier-only hold-to-talk (Shift+Control by default, configurable via `CGEventTap`, non-blocking)
- **Model management** — download, list, remove whisper models from CLI or TUI

## Install

```bash
cargo install miccli
```

Or build from source:

```bash
git clone https://github.com/AkashJana18/miccli.git
cd miccli
cargo build --release
# binary at target/release/miccli (24 MB release, 62 MB debug)
```

## Quick Start

```bash
# Download a whisper model (first time only)
miccli models download small

# Start listening (TUI by default)
miccli start
# ↳ Live tab shows waveform + transcription; hold Shift+Control, speak, release

# Plain log mode (no TUI, e.g. inside a nested TUI you want to dictate into)
miccli start --no-tui
```

Hold **Shift+Control** (default hotkey), speak, release. Text appears in your active app. Use `Tab`/`1-4` to switch tabs, `q` or `Ctrl+C` to quit. The hotkey is global — it works even when the TUI has focus, so run miccli in a **separate pane/window** from the app you dictate into.

> **Permissions** — macOS will ask for **Microphone** access, and you must grant miccli
> **Accessibility** (System Settings → Privacy & Security → Accessibility) so it can detect
> the hotkey and insert text into other apps. Without it the hotkey won't fire (`TapDisabledByUserInput`).

## TUI

`miccli start` launches `ratatui` with `crossterm` (enabled by default, auto-detects TTY; falls back when `stdout` is not a terminal or with `--no-tui`).

```
┌ miccli — voice dictation ────────────────────────────────────  ◉ miccli v0.2.0  ● REC  Shift+Control ─┐
│ ◐ auto  dev.opencode  ● model ready                                                              │
├──────────────────────────────────────────────────────────────────────────────────────────────────┤
│  ◉ Live    │  ◈ Models    │  ⚙ Config    │  ? Help                                              │
├──────────────────────────────────────────────────────────────────────────────────────────────────┤
│ waveform — ● recording  48210 samples  peak 68%   (sparkline, cyan→yellow→red by amplitude)      │
│ transcription — listening…  "open curly brace hello world close curly brace."                    │
│ pipeline  transcribe  820ms  cleanup 120ms  insert 30ms  total 970ms  │ insertion  ⌨ type 20ms/char│
├──────────────────────────────────────────────────────────────────────────────────────────────────┤
│ q quit  tab switch  1-4 jump  ⇧^ hold talk │ live: waveform + transcript │ miccli v0.2.0  •  ● REC │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Tabs:**
- **Live** — waveform (RMS+peak blended `0..100`, decay-gated, width 60–220 cols), transcription (cleaned + raw), `● REC`/`■ idle` pulsing, `app_name` + `strategy`, latencies, VAD status.
- **Models** — table `tiny 75 MB`/`base 142 MB`/`small 466 MB`/`medium 1.5 GB` with `✅ downloaded`/`○ not` and `▶` active marker; `miccli models download <name>` to fetch.
- **Config** — highlighted `~/.config/miccli/config.toml` (`[sections]` magenta, keys cyan).
- **Help** — keys, insertion explainer, and permission checklist.

**Keys:** `q`/`Esc`/`Ctrl+C` quit · `Tab`/`Shift+Tab` next/prev · `1` `2` `3` `4` jump. Runs at 30 fps idle / 80 fps recording (`tokio::time::sleep`, non-blocking `HotkeyManager::try_action`).

## Commands

| Command | Description |
|---------|-------------|
| `miccli start` | Start daemon with TUI (default) — `Shift+Control` hold to talk; `q` quit |
| `miccli start --no-tui` | Start daemon with plain log output (`● recording…` / `■ stopped`) |
| `miccli stop` | Stop daemon (removes `~/.config/miccli/miccli.pid`) |
| `miccli toggle` | Toggle recording on/off (sends `SIGUSR1`) |
| `miccli config` | Show config path, hint to edit |
| `miccli config --path` | Print config path only |
| `miccli models` | List/download/remove whisper models (also visible in TUI Models tab) |
| `miccli models download <size>` | Download a model (tiny/base/small/medium) |
| `miccli models list` | List installed models |

## Config

Config lives at `~/.config/miccli/config.toml`:

```toml
[hotkey]
key = ""                      # main key, or "" / "None" for modifier-only hold
modifier = "Shift+Control"    # Command | Option | Control | Shift, joined with "+"

[whisper]
model = "small"               # tiny | base | small | medium
language = "en"               # ISO 639-1 code
metal = true                  # Apple Metal acceleration

[vad]
threshold = 0.5               # Voice activity detection threshold
min_speech_ms = 250
min_silence_ms = 500

[llm]
provider = "ollama"           # ollama | groq | openai | none
model = "qwen2.5:1.5b"       # Model name
enabled = false               # Enable LLM cleanup

[insertion]
default = "auto"              # auto | type | clipboard
key_delay_ms = 20             # Delay between keystrokes (type mode)
paste_delay_ms = 10
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

The `auto` mode (default) detects the frontmost app via `osascript` and picks the right strategy (`src/insert/mod.rs`, `src/insert/app_detect.rs`). You can override per-app in config, or force a global strategy. Because every terminal emulator is mapped to `type`, **all nested terminal CLIs work**: `opencode`, `nvim`, `codex` inside Kitty/Alacritty/iTerm2/Terminal/Ghostty/WezTerm are typed char-by-char without collapse.

> **Nested-CLI tip:** Run `miccli start` in a **separate pane/window** from the TUI you dictate into. The insertion uses `CGEvent` to type into the frontmost app — if miccli's own TUI is frontmost, text would go to its own terminal instead of `opencode`/`nvim`. The hotkey is global (`CGEventTap` in `src/hotkey.rs`), so you can hold `Shift+Control` while `opencode` is focused even though `miccli`'s TUI is in another pane.

For VS Code's integrated terminal (`com.microsoft.VSCode` defaults to `paste`), add:

```toml
[[insertion.apps]]
bundle_id = "com.microsoft.VSCode"
strategy = "type"
```

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
cargo build --release  # 24 MB, lto+strip; deps: whisper-rs + ort + ratatui/crossterm
cargo test             # 51 passed (incl. TUI TestBackend renders)
```

Release size: `ratatui 0.29` + `crossterm 0.28` adds ~1–2 MB over the `ort`/`whisper-rs` baseline (still 24 MB release).

## License

MIT
