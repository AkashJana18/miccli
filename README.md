# miccli

Terminal voice dictation CLI — local Whisper STT + code-aware cleanup + smart text insertion.

Hold a hotkey, speak, release. Your voice becomes text, instantly in your terminal. **Overlay appears only while recording** like WhisperFlow — terminal stays blank when idle.

**Why miccli exists:** Claude Code, Codex, and opencode collapse pasted multi-line text into `[Pasted text]`. miccli detects this and types character-by-character instead, so everything arrives intact. It works in every nested terminal CLI (opencode, nvim, etc.) because terminal emulators are classified as `type` (see below).

## Features

- **WhisperFlow overlay (ratatui)** — `miccli start` is blank when idle; hold `Shift+Control` → small top bar with live waveform (blocks `▁▂▃▄▅▆▇█`) and transcription appears, auto-hides on release. No `q` needed while coding.
- **Full dashboard** — `miccli dashboard` stops the daemon, opens the 4-tab TUI (Live waveform sparkline, Models, Config, Help), `q` to quit and auto-restarts the overlay daemon. Both modes coexist.
- **Local Whisper STT** — no API key needed, runs offline via whisper-rs + Metal acceleration
- **App-aware text insertion** — detects frontmost app, routes to slow char typing (TUIs) or fast clipboard paste (editors)
- **Two-tier cleanup** — regex rules for 56+ symbols (instant), optional LLM polish via Ollama/Groq
- **Global hotkey** — modifier-only hold-to-talk (Shift+Control by default, `CGEventTap`, non-blocking)
- **Pause toggle** — `miccli toggle` pauses/resumes dictation (SIGUSR1)
- **Background daemon** — `miccli start --background` (`-b`) returns immediately, daemon stays alive with global overlay (double-fork), logs to `~/.config/miccli/miccli.log` (or `--log-file`)
- **Status & restart** — `miccli status` checks PID + log, detects stale PID; `miccli restart [--background]` stops then starts (duplicate guard prevents double start)
- **Model management** — download, list, remove whisper models from CLI or dashboard

## Install

```bash
cargo install miccli
# or before crates.io publish:
cargo install --git https://github.com/AkashJana18/miccli
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

# Start overlay daemon (blank when idle, overlay on hotkey)
miccli start
# ↳ hold Shift+Control, speak, release — overlay shows waveform + result, then hides

# Start in background (returns immediately, overlay stays global, logs to ~/.config/miccli/miccli.log)
miccli start --background  # or -b, --log-file /tmp/miccli.log

# Check status (PID + log tail, detects stale PID)
miccli status

# Full dashboard for config/model management
miccli dashboard
# ↳ 4 tabs: Live / Models / Config / Help — q to quit and daemon restarts

# Pause / resume without stopping
miccli toggle

# Restart (stop if running, then start; supports --background)
miccli restart --background
```

Hold **Shift+Control** (default hotkey), speak, release. Text appears in your active app. The hotkey is global — it works even when the overlay terminal is in the background, so run `miccli start` in a **separate pane/window** from the app you dictate into (e.g. `opencode`, `nvim`).

> **Permissions** — macOS will ask for **Microphone** access, and you must grant miccli
> **Accessibility** (System Settings → Privacy & Security → Accessibility) so it can detect
> the hotkey and insert text into other apps. Without it the hotkey won't fire (`TapDisabledByUserInput`).

## TUI Modes

### Overlay (default — whisperflow)

`miccli start` with `[tui] mode = "overlay"` (default). Terminal is **completely blank when idle** — no rendering, no clear, primary screen unchanged. On `Shift+Control` press:

```
┌─ miccli ● REC  Shift+Control  dev.opencode  ⌨ type ──────────────┐
│ ███▓▓▓█▓▓▓▓█▓▓▓▓▓█▓▓▓█▓▓  68%                                  │
│ "open curly brace function fetch close curly brace"               │
│ ··· 970ms total · typing into opencode ·                          │
└───────────────────────────────────────────────────────────────────┘
```

- Top-aligned, ~6 lines, rounded border, pulsing `● REC` / `■ IDLE`, single-line blocks waveform (RMS+peak `0..100`, color cyan→yellow→red), live transcription, latency/status.
- Auto-hides ~1.4 s after release (shows result briefly). No `q` needed.
- When `paused` (`miccli toggle`), shows `⏸ paused` and ignores hotkey until toggled again.
- CPU: `50 ms` sleep when idle (~20 Hz, ~0.1% CPU), `12 ms` when recording (~80 Hz) — same as before, foreground vs background identical (daemon work dominates). Memory ~24 MB.

### Dashboard (full)

`miccli dashboard` — stops the running daemon (if any), opens full 4-tab TUI, `q` to quit and daemon auto-restarts (same terminal becomes overlay daemon).

```
┌ miccli — voice dictation ───────────────────  ◉ miccli v1.0.0  ● REC  Shift+Control ─┐
│ ◐ auto  dev.opencode  ● model ready                                                     │
├─────────────────────────────────────────────────────────────────────────────────────────┤
│  ◉ Live    │  ◈ Models    │  ⚙ Config    │  ? Help                                     │
├─────────────────────────────────────────────────────────────────────────────────────────┤
│ waveform — ● recording  48210 samples  peak 68%  (sparkline, cyan→yellow→red)             │
│ transcription — listening…  "open curly brace hello world close curly brace."           │
│ pipeline  transcribe  820ms  cleanup 120ms  insert 30ms  total 970ms │ insertion ⌨ type  │
├─────────────────────────────────────────────────────────────────────────────────────────┤
│ q quit  tab switch  1-4 jump  ⇧^ hold talk │ live: waveform + transcript │ v1.0.0 ● REC  │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

**Tabs:**
- **Live** — sparkline waveform (60–220 cols), transcription (cleaned + raw), `app_name` + `strategy`, latencies, VAD status.
- **Models** — table `tiny 75 MB`/`base 142 MB`/`small 466 MB`/`medium 1.5 GB` with `✅`/`○` and `▶`.
- **Config** — highlighted `~/.config/miccli/config.toml`.
- **Help** — keys, insertion explainer, permission checklist.

**Keys (dashboard only):** `q`/`Esc`/`Ctrl+C` quit · `Tab`/`Shift+Tab` next/prev · `1` `2` `3` `4` jump. Runs at 30 fps idle / 80 fps recording.

### `tui.mode` config

```toml
[tui]
mode = "overlay"   # overlay | dashboard | none
# overlay — minimal top bar only while recording (default)
# dashboard — persistent 4-tab when `miccli start` (or just use `miccli dashboard`)
# none — plain logs (`● recording…` / `■ stopped`) even when TTY
```
Automatically falls back to plain when `stdout` is not a TTY.

## Commands

| Command | Description |
|---------|-------------|
| `miccli start` | Start overlay daemon (blank idle, whisperflow on hotkey) — blocks |
| `miccli start --background` | Same but daemonize (double-fork, returns immediately, overlay stays global, log `~/.config/miccli/miccli.log` or `--log-file`) |
| `miccli status` | Show daemon PID / PID file / log size + tail, detect stale PID |
| `miccli restart [--background]` | Stop if running (SIGTERM, wait) then start (same flags as start) |
| `miccli dashboard` | Stop daemon → open full dashboard → `q` → daemon restarts |
| `miccli toggle` | Pause/resume daemon (⏸, `SIGUSR1`) |
| `miccli stop` | Stop daemon (`SIGTERM`, removes pid, panic-safe via `PidFileGuard`) |
| `miccli config` | Show config path, hint to edit |
| `miccli config --path` | Print config path only |
| `miccli models` | List/download/remove whisper models (also in dashboard) |
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

[tui]
mode = "overlay"              # overlay | dashboard | none
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

The `auto` mode (default) detects the frontmost app via `osascript` and picks the right strategy (`src/insert/mod.rs`, `src/insert/app_detect.rs`). Because every terminal emulator is mapped to `type`, **all nested terminal CLIs work**: `opencode`, `nvim`, `codex` inside Kitty/Alacritty/iTerm2/Terminal/Ghostty/WezTerm are typed char-by-char without collapse.

> **Nested-CLI tip:** Run `miccli start` in a **separate pane/window** from the TUI you dictate into. The insertion uses `CGEvent` to type into the frontmost app — the hotkey is global (`CGEventTap` in `src/hotkey.rs`), so you can hold `Shift+Control` while `opencode` is focused even though `miccli`'s overlay is blank in another pane. `miccli dashboard` needs its own terminal — it stops the daemon first.

For VS Code's integrated terminal (`com.microsoft.VSCode` defaults to `paste`), add:

```toml
[[insertion.apps]]
bundle_id = "com.microsoft.VSCode"
strategy = "type"
```

## LLM Cleanup

Optional code-aware cleanup via local or cloud LLMs — **opt-in, disabled by default**. On first run, miccli prompts:

```
miccli first run — LLM cleanup is disabled by default (opt-in).
  1) No — keep disabled (default)
  2) Ollama — local, free
  3) Groq — cloud, BYOK
  4) OpenAI — cloud, BYOK
```

Your choice is saved to `~/.config/miccli/config.toml`. You can also set `MICCLI_NO_PROMPT=1` for non-interactive installs.

**Ollama (free, local):**
```bash
ollama pull qwen2.5:1.5b
```
Then in config: `provider = "ollama"`, `enabled = true`

**Groq (free tier, BYOK):**
```bash
export GROQ_API_KEY=gsk_...
```
Then in config: `provider = "groq"`, `enabled = true`

**OpenAI (BYOK):**
```bash
export OPENAI_API_KEY=sk-...
```
Then in config: `provider = "openai"`, `enabled = true`

**How it works:**
1. Regex rules run first (instant, handles ~60% of cases)
2. LLM runs only if text > 20 chars and not just a symbol (fast path)
3. LLM receives user config + dictation context for code-aware results

## Building from Source

Requirements:
- Rust 1.80+
- macOS 13+ (Apple Silicon recommended, Metal acceleration; macOS only)

```bash
cargo build --release  # 24 MB, lto+strip; deps: whisper-rs + ort + ratatui/crossterm
cargo test             # 56 passed, 3 ignored (incl. TUI TestBackend renders)
```

Release size: `ratatui 0.29` + `crossterm 0.28` adds ~1–2 MB over the `ort`/`whisper-rs` baseline (still 24 MB release).

**Background vs foreground:** `miccli start` blocks (blank when idle) by default. `miccli start --background` (`-b`) daemonizes via double-fork (`fork` → `setsid` → `fork` → `dup2` log) — parent exits immediately, overlay stays global (native `NSWindow` level 1000 on macOS), stdout/stderr appended to `~/.config/miccli/miccli.log` (or `--log-file`). Without `--background`, CPU is identical for `overlay` vs `dashboard` vs plain (`50 ms` idle, `12 ms` recording). `miccli dashboard` reuses the same terminal via `SIGTERM` + `EnterAlternateScreen`; `toggle` (`SIGUSR1`) pauses with near-zero cost. `miccli status` checks PID via `kill(pid,0)` and detects stale PID; duplicate `miccli start` bails with `already running`. `miccli stop` / `toggle` / `dashboard` / `restart` all work via PID file `~/.config/miccli/miccli.pid` + `libc::kill` whether daemon is backgrounded or not; PID file is panic-safe via `PidFileGuard` + `panic::set_hook`.

## License

MIT
