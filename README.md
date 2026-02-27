# Clipboard Guard

A cross-app clipboard security tool that monitors copy/paste operations and enforces rules to prevent accidental data leaks between applications.

## Features

- **Clipboard monitoring** — detects which app placed content on the clipboard
- **Cross-app paste warnings** — notifies when pasting between apps with active rules
- **Paste blocking** — optionally blocks Cmd+V for configured app pairs (requires Accessibility permission)
- **Custom rules** — configure per-app source/destination pairs with notify or block actions
- **System tray** — runs as a menu bar app with quick toggle

## Download

Grab the latest build from the [Releases](../../releases) page.

| Platform | Format |
|----------|--------|
| macOS (Apple Silicon) | `.dmg` |
| macOS (Intel) | `.dmg` |
| Windows | `.msi` / `.nsis` |
| Linux | `.deb` / `.AppImage` |

> **Note:** Clipboard monitoring and paste blocking are currently macOS-only. Windows and Linux builds compile and run but monitoring is a no-op.

## Development

```bash
pnpm install
pnpm tauri dev
```

## Tech

Tauri 2 + React + TypeScript. Rust backend with macOS-native clipboard monitoring via `objc2` and `CoreGraphics` event taps.
