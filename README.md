![Windows](https://img.shields.io/badge/platform-Windows-blue)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

# Claude Code Usage Bubble

A floating, draggable circular bubble that shows your Claude Code and/or
Codex usage on Windows — inspired by the floating "memory boost ball" UX
of 360 Security and IObit Advanced SystemCare.

Drop it anywhere on screen, drag it around, snap it to a monitor edge,
left-click for a panel with both your 5-hour and 7-day windows, right-click
for the menu.

## Differences vs upstream

This project is a derivative of
[CodeZeno/Claude-Code-Usage-Monitor](https://github.com/CodeZeno/Claude-Code-Usage-Monitor)
(MIT, © 2026 Code Zeno Pty Ltd). The usage-polling, updater, tray-icon,
localization, theme-detection, and diagnostic modules are ported from that
codebase with minor adaptations.

The original app embeds a horizontal widget directly into the Windows
taskbar. This fork replaces that UI with a **floating circular bubble that
the user can drag anywhere on screen**, plus an on-demand expanded panel.
Everything else (credential reading, OAuth refresh via the Claude/Codex
CLI, WSL credential support, GitHub self-update, eight languages) behaves
the same.

## What you get

- A circular floating bubble showing your current 5-hour Claude Code or
  Codex usage as a percentage and a colored progress ring
- Drag anywhere — the bubble snaps to monitor work-area edges when
  released
- Resize with `Ctrl + MouseWheel` on the bubble (32–128 pixels)
- Left-click the bubble for an expanded panel with both **5h** and **7d**
  bars plus reset countdowns
- Right-click for refresh, displayed models, update frequency, language,
  startup, updates, exit
- Optional system tray icons (one per enabled model)
- Auto-hide when a fullscreen app is in the foreground (games, video,
  presentations) — reappears when you leave fullscreen

## Who this is for

Windows 10/11 users who already have **Claude Code (CLI or App) installed
and signed in**. Codex support is optional — install and sign in to the
Codex CLI, then enable Codex from the right-click **Models** menu.

If you use Claude Code through WSL, that is supported too. The monitor
can read your Claude Code credentials from Windows or from your WSL
environment.

## Requirements

- Windows 10 or Windows 11
- Claude Code (CLI or App) installed and authenticated
- Optional: Codex CLI installed and authenticated, if you want Codex usage

## Install

Until packaged binaries are published, build from source:

```powershell
git clone https://github.com/<your-fork>/claude-code-usage-bubble
cd claude-code-usage-bubble
cargo build --release
```

The binary lands at `target/release/claude-code-usage-bubble.exe`.

## Use

Run `claude-code-usage-bubble.exe`. The bubble appears near the bottom-right
corner of your primary monitor on first launch. Drag it where you want it,
release to snap to the nearest edge if you let go close to one.

- **Left-click** the bubble to open the expanded panel (5h + 7d + countdowns)
- **Right-click** for refresh, models, update frequency, language, "Start
  with Windows", updates, exit
- **Drag** anywhere — it floats on top of all other windows
- **Ctrl + MouseWheel** on the bubble to resize it
- **Tray icon** (if enabled): left-click toggles the bubble visibility,
  right-click opens the same menu

### Models

Use the right-click **Models** menu to choose what is shown:

- **Claude Code** is enabled by default
- **Codex** can be enabled alongside Claude Code or shown by itself

When both models are shown, each gets its own bubble that you can position
independently.

## Diagnostics

```powershell
claude-code-usage-bubble.exe --diagnose
```

This writes a log file to:

```text
%TEMP%\claude-code-usage-bubble.log
```

Settings are saved to:

```text
%APPDATA%\ClaudeCodeUsageBubble\settings.json
```

## Privacy and security

What the app reads:

- Your local Claude Code OAuth credentials from `~/.claude/.credentials.json`
- If needed, the same credentials file inside an installed WSL distro
- If Codex is enabled, your local Codex credentials from `$CODEX_HOME/auth.json`
  or `~/.codex/auth.json`

What the app sends over the network:

- Requests to Anthropic's Claude endpoints to read your usage
- Requests to ChatGPT's Codex usage endpoint, if Codex is enabled
- Requests to GitHub only if you use the app's update-check feature

What the app stores locally:

- Bubble position(s) per model
- Bubble size
- Polling frequency
- Language preference
- Last update check time
- Displayed model preferences

What it does **not** do: send credentials to any third-party server, run a
backend service, collect analytics, upload your project files, or write to
your Codex `auth.json` directly.

## License

Apache License 2.0 — see [LICENSE](LICENSE). This project is a derivative of
[CodeZeno/Claude-Code-Usage-Monitor](https://github.com/CodeZeno/Claude-Code-Usage-Monitor)
(MIT). Upstream attribution and the original MIT terms for the ported portions
are recorded in [NOTICE](NOTICE).
