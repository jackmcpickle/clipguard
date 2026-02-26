# Progress

## Completed

### Phase 1 — Project Setup
- Configured as menu bar app (no dock icon via `ActivationPolicy::Accessory`)
- System tray with Show/Toggle/Quit menu
- `tauri-plugin-autostart` with LaunchAgent support
- `image-png` feature for tray icon fallback

### Phase 2 — Clipboard Monitoring
- Polls `NSPasteboard.changeCount` every 300ms via `objc2-app-kit`
- Emits `clipboard-changed` events to frontend
- Stores copy source app (bundle ID + name)

### Phase 3 — Active App Detection
- Uses `NSWorkspace.frontmostApplication` to track active app
- Single call per loop iteration to avoid race conditions
- Bundle ID + localized name extraction from `NSRunningApplication`

### Phase 4 — Cross-App Paste Detection
- Detects app switches to monitored terminals
- Sends macOS native notification when clipboard source differs from destination
- Monitored: Terminal, iTerm2, Alacritty, WezTerm, Kitty, Hyper, Ghostty, Rio
- Deduplication: won't re-warn for same (source, dest) pair until new clipboard copy

### Phase 5 — Settings & UX
- Tray menu: toggle guard on/off (updates menu label), Settings window, Quit
- Settings window: guard toggle, autostart toggle, last clipboard source, recent warnings list
- React frontend with clean macOS-native styling
- `get_enabled`/`set_enabled` Tauri commands
- Frontend listens to `clipboard-changed` and `paste-warning` events

## Remaining

### Phase 6 — Packaging & Distribution
- Code signing with Apple Developer ID
- Notarization via `xcrun notarytool`
- `.dmg` packaging
- Optional: Homebrew cask formula

## Decisions
- Alert on app-switch (not Cmd+V interception) — simpler, no Accessibility API needed for v1
- NSPasteboard/NSWorkspace called from background thread with documented trade-off
- 300ms poll interval per PLAN.md guidance

## Next Step
- `pnpm tauri build` to produce a working .app bundle for local testing
