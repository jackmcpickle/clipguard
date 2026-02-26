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
- Deduplication: won't re-warn for same (source, dest) pair until new clipboard copy

### Phase 5 — Settings & UX
- Tray menu: toggle guard on/off (updates menu label), Settings window, Quit
- Settings window: guard toggle, autostart toggle, last clipboard source, recent warnings list
- React frontend with clean macOS-native styling

### Phase 6 — Packaging & Distribution (partial)
- Renamed productName to "Clipboard Guard", identifier to `com.jackmcpickle.clipboard-guard`
- Bundle targets narrowed to `["dmg", "app"]` (macOS only)
- Added `Entitlements.plist` (sandbox disabled, apple-events enabled)

### Phase 7 — Settings Window + Configurable Rules
- **Task switcher visibility**: Window shows in Cmd+Tab when open, hides on close (activation policy toggle + `on_window_event` close prevention)
- **Rules data model** (`rules.rs`): `BlockRule` with from/to app + action (notify/block), JSON persistence at `app_data_dir/rules.json`, defaults from old hardcoded terminal list
- **Clipboard monitoring refactored** (`clipboard.rs`): Removed hardcoded `MONITORED_DESTINATIONS`, uses configurable rules with `matches_rule()`. Block action shows "(requires Accessibility)" in notification
- **New commands**: `get_rules`, `set_rules`, `pick_app` (folder picker reads Info.plist for bundle ID), `check_accessibility` (AXIsProcessTrusted FFI), `open_accessibility_settings`
- **App picker**: `tauri-plugin-dialog` folder picker defaulting to `/Applications`, reads `CFBundleIdentifier`/`CFBundleName` from Info.plist
- **Frontend rules UI**: Rules editor with from/to app browse, notify/block toggle, add/remove, validation (both wildcards = error), accessibility permission banner
- **Config**: Window height 600, resizable, dialog plugin added

### Phase 8 — CGEventTap Paste Blocking + Accessibility Polling
- **CGEventTap FFI** (`clipboard.rs`): Raw CoreGraphics bindings for `CGEventTapCreate`, `CGEventTapEnable`, `CGEventGetFlags`, etc.
- **Blocker thread**: Dedicated thread with its own CFRunLoop, receives Enable/Disable messages via `mpsc::channel` from monitor thread
- **Tap callback**: Intercepts `kCGEventKeyDown`, suppresses Cmd+V and Cmd+Shift+V (keycode 9 + Command flag → return null)
- **Monitor loop integration**: Block rule + accessibility → sends Enable, notification "Paste blocked: X → Y". Switch away / clipboard change / guard disabled → sends Disable
- **Fallback**: Block rule without accessibility → falls back to notify with accessibility warning
- **Accessibility polling** (`App.tsx`): `getCurrentWindow().onFocusChanged()` re-checks `AXIsProcessTrusted` on every window focus, re-shows banner if revoked
- **PasteWarning.blocked**: New field distinguishes actual blocks from notifications
- **Tap lifecycle**: Created on block activation, torn down on app switch / clipboard change / disable / channel disconnect
- **App picker bug fix**: Already uses `blocking_pick_file` with `.app` filter (was fixed in previous session)

## Decisions
- Alert on app-switch (not Cmd+V interception) — simpler, no Accessibility API needed for v1
- `.app` bundles are directories — use `blocking_pick_folder` not `blocking_pick_file`
- `AXIsProcessTrusted` via extern "C" FFI — no extra crate needed
- 300ms poll interval per PLAN.md guidance
- No Developer Program — ad-hoc signing
- CGEventTap on dedicated thread — needs own CFRunLoop, can't share with monitor thread
- One notification per block session — tap callback silently suppresses, no per-keypress notification
- Block disabled on clipboard change — new content needs re-evaluation on next switch

## Remaining
- Custom app icon
- Runtime testing: verify tap actually blocks paste in Terminal, iTerm2, etc.
- Edge case: tap may get disabled by macOS if app loses trust — may need periodic re-enable check

## Next Step
- Test with `pnpm tauri dev` — verify Cmd+V blocked in blocked apps, notify-only rules still work, accessibility polling on window focus
