# Cross-Platform Framework Options

Researched 2026-10-21. Scope: run the bubble on Windows, macOS, and Linux from one codebase.

## Answer

Use **Tauri v2**. It keeps the project in Rust, so the provider, credential, settings, and usage-model code (`src/usage/`, `src/creds/`, `src/settings.rs`, about 1,500 lines) moves over with small changes. Tauri covers the parts that are Win32-only today: a transparent, frameless, always-on-top window, the tray, and an updater plugin. The bubble and panel drawing in `src/bubble.rs` and `src/panel.rs`, about 2,600 lines of tiny-skia and GDI code, would be rewritten as HTML, CSS, and SVG. A circle with a progress ring is simple to draw that way.

No framework removes all per-OS work. The OS limits below apply to every option.

## Current state

The app is 7,500 lines of Rust written directly against Win32 through the `windows` crate. The Win32-bound modules are `bubble.rs`, `app.rs`, `panel.rs`, `tray/`, `net/winhttp.rs`, `os/`, and `update/install.rs`. The portable modules are the provider clients, the credential parsing, the settings, and the usage types.

## Options compared

| Option | Language | What carries over | Fit for a small floating widget | Maturity |
| --- | --- | --- | --- | --- |
| **Tauri v2** | Rust + web UI | All non-UI Rust | Good. It has transparent, `alwaysOnTop`, `skipTaskbar`, tray, and updater support. Uses the system webview, so the binary stays small. | Stable, with releases through mid-2026 |
| winit + tiny-skia + `tray-icon` + `muda` | Rust | Non-UI Rust plus the tiny-skia renderer | Good in theory, but you build drag, DPI, menus, and updates yourself. Transparent presentation through `softbuffer` is uneven across platforms (unverified). | These are the crates Tauri uses internally |
| egui / eframe | Rust | Non-UI Rust | Fair. There are open reports of transparent windows rendering black, and tray support is not built in. | Stable, but the API changes often |
| Wails v3 | Go | Nothing. This is a full rewrite. | Good. It has frameless and transparent windows plus a tray with an attached window. | **Beta** (v3.0.0-beta.x), and the docs still say alpha |
| Electron | JS | Nothing. This is a full rewrite. | Works, but it ships about 100 MB of Chromium for a 100 px circle. | Stable |

Wails v3 is the best match for your language ranking (Go first). It is still beta, though, and it means throwing away working Rust. Tauri gives you cross-platform support without a rewrite.

## Limits that apply to every framework

- **Linux Wayland:** Wayland does not let an app place its own window or keep it always on top. There is no core protocol for either, and wlr-layer-shell exists only on some compositors. On GNOME and KDE Wayland, the bubble's drag-to-position, edge snapping, and "always on top" will fail or do nothing. The workarounds are to run the app under XWayland (for example `GDK_BACKEND=x11`) or to treat Wayland as best-effort.
- **macOS credentials:** Claude Code stores its token in the macOS Keychain, not in a file. Third-party code reports the Keychain service name as `Claude Code-credentials`; check that on a real Mac. Linux uses `~/.claude/.credentials.json`, the same as Windows. `src/creds/` needs a Keychain backend, for example the `keyring` or `security-framework` crate.
- **macOS transparency:** Tauri needs `macOSPrivateApi` plus the `macos-private-api` Cargo feature. That rules out the Mac App Store, which a GitHub-release app doesn't need. One open issue reports transparency working in `tauri dev` but rendering white in a bundled build.
- **Fullscreen auto-hide:** Detecting a fullscreen foreground app is OS-specific under any framework. Expect a small `#[cfg(target_os)]` module for each OS.
- **Self-update:** Each OS needs its own approach. Windows replaces the .exe, macOS replaces the .app bundle, and Linux replaces an AppImage (Tauri's updater supports all three). The updater requires signed update artifacts. Unsigned macOS builds also hit Gatekeeper, which is stricter than SmartScreen, so you need an Apple Developer ID ($99 a year) to ship macOS properly.
- **Startup entry:** The Windows registry Run key becomes a LaunchAgent on macOS and an XDG autostart file on Linux. The Tauri `autostart` plugin handles all three.

## Suggested migration path

```text
1. Split crate: core (usage, creds, settings; swap WinHTTP -> ureq/reqwest) + platform shell
2. Add Tauri v2 shell: transparent always-on-top window, tray, autostart, updater plugins
3. Port bubble/panel rendering to HTML/CSS/SVG; drag via data-tauri-drag-region or startDragging()
4. Per-OS shims: macOS Keychain creds, fullscreen detection
5. CI matrix: windows / macos / ubuntu builds + signed updater manifests
```

Step 1 is worth doing whichever framework you choose, because it isolates the provider logic from the UI.

## Next steps

1. Spike Tauri v2 for a day. Build only a transparent, round, draggable, always-on-top window with a tray icon, and test it on Windows, macOS, and Linux (both X11 and Wayland). Decide based on that result, not on the docs.
2. If the spike passes, plan the core/shell split before porting any UI.

## Unresolved questions

- Is macOS in scope, and will you pay for an Apple Developer ID? Without one, macOS distribution is painful.
- Which Linux targets matter? Is Wayland-first GNOME/KDE in scope, or is X11/XWayland acceptable?
- Would you accept a Go rewrite (Wails v3, still beta) to follow your language preference, or is keeping Rust the priority?

## Follow-up decisions (2026-10-21)

The user set the platform priority to Windows first, then Linux, then macOS. macOS and Wayland support are optional, but the stack must allow porting to them later. The user will not buy an Apple Developer ID and is open to either Rust or Go.

- **Unsigned macOS can still ship.** On macOS 15 Sequoia and later, Control-click → Open no longer bypasses Gatekeeper. Users open the app once, then click **Privacy & Security → Open Anyway**, or run `xattr -d com.apple.quarantine <app>`. Tauri's updater signs updates with its own free key pair, so it does not need an Apple certificate.
- **Wails v3 is still in beta.** The latest release found is v3.0.0-beta.12, whose notes say the API is stable. On macOS and Linux it needs cgo (Cocoa and WebKitGTK bindings), which breaks the pure-Go, `CGO_ENABLED=0` rule. On Windows it does not need cgo.

## Sources

- [Tauri release history](https://tauri.app/release/tauri/all-versions/)
- [Tauri v2 window configuration notes](https://dev.to/rain9/tauri-part-3-get-the-window-configuration-right-first-3gf7)
- [Tauri issue: transparency white in bundled macOS build](https://github.com/tauri-apps/tauri/issues/13415)
- [Wails v3 what's new](https://v3.wails.io/ru/whats-new), [Wails v3 frameless windows](https://v3.wails.io/features/windows/frameless/), [Wails v3.0.0-beta.2](https://newreleases.io/project/github/wailsapp/wails/release/v3.0.0-beta.2)
- [egui transparency discussion](https://de.github.com/emilk/egui/discussions/4446)
- [Wayland: always-on-top protocol request](https://gitlab.freedesktop.org/wayland/wayland/-/issues/305), [Wayland: can't set top-level position](https://gitlab.freedesktop.org/wayland/wayland/-/issues/183)
- [Claude Code authentication docs](https://code.claude.com/docs/en/authentication.md), [ai-usagebar keychain source](https://docs.rs/crate/ai-usagebar/0.21.0/source/src/anthropic/keychain.rs)
