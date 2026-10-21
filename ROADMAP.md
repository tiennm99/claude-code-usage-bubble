# Roadmap

These items are undecided or planned for later. None of them is part of the
current Tauri port.

- **Linux ARM64 builds.** Publish aarch64 Linux builds, including an AppImage,
  and add the `linux-aarch64` entry to the updater's `latest.json`. Linux ships
  x86_64 only for now.
- **macOS build.** Ship an unsigned macOS build. The install notes must explain
  how to get past Gatekeeper with "Open Anyway" in System Settings, since the
  app is not notarized.
- **Wayland-native positioning.** Position, snap and keep the bubble on top
  under native Wayland. Today these work only through Xwayland.
- **Repository and product rename.** A new name to replace
  "claude-code-usage-bubble" is under discussion. Any rename must keep the
  legacy update asset names and the GitHub redirects working, so that installed
  copies can still find and download updates.
