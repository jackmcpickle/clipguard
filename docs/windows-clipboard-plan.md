# Plan: Windows clipboard monitoring + app listing

## Context

Clipboard Guard runs on Windows but the clipboard stub is a no-op — no monitoring, no frontmost-app detection, no paste blocking, and `list_apps` returns empty. Need real Windows implementations for all three + registry-based app listing.

Approach: mirror the macOS polling architecture using the official Microsoft `windows` crate for Win32 APIs, and `winreg` for registry-based app enumeration.

---

## Step 1: Add Windows dependencies to `Cargo.toml`

Add a new Windows-specific deps section:

```toml
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.62", features = [
  "Win32_System_DataExchange",
  "Win32_UI_WindowsAndMessaging",
  "Win32_System_Threading",
  "Win32_Foundation",
  "Win32_UI_Input_KeyboardAndMouse",
  "Win32_System_LibraryLoader",
] }
winreg = "0.55"
```

**File:** `src-tauri/Cargo.toml`

---

## Step 2: Create `src-tauri/src/clipboard_windows.rs`

Full Windows clipboard module replacing the stub. Same public API as `clipboard.rs`:
- `ClipboardEvent`, `PasteWarning`, `ClipboardState` (same types)
- `start_clipboard_monitor(app, state)` spawns background threads

### Clipboard change detection (polling)
- `GetClipboardSequenceNumber()` — returns `u32` that increments on clipboard change
- Poll every 300ms, same as macOS `NSPasteboard.changeCount`

### Frontmost app detection
- `GetForegroundWindow()` → HWND
- `GetWindowThreadProcessId(hwnd, &mut pid)` → PID
- `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)` → handle
- `QueryFullProcessImageNameW(handle, ...)` → exe path → extract exe name
- **`source_app_id`** = exe filename (e.g. `"msedge.exe"`) — matches registry app listing
- **`source_app_name`** = exe stem (e.g. `"msedge"`) as fallback; ideally map to registry `DisplayName` if available

### Paste blocking
- Install `SetWindowsHookExW(WH_KEYBOARD_LL, callback, hinstance, 0)` once on blocker thread startup
- Callback checks: `wParam == WM_KEYDOWN` and `vkCode == VK_V` with `GetAsyncKeyState(VK_CONTROL) < 0`
- Gate blocking with atomic flag set by monitor thread (`Enable`/`Disable` messages)
- Return non-zero to suppress the keypress
- Needs message pump: `GetMessageW` loop on blocker thread
- Teardown: `UnhookWindowsHookEx` on thread exit

### Thread architecture (same as macOS)
1. **Monitor thread** — polls clipboard + foreground app, emits events, sends block/unblock
2. **Blocker thread** — receives Enable/Disable via channel, installs/removes keyboard hook + message loop

**File:** `src-tauri/src/clipboard_windows.rs`

---

## Step 3: Update `lib.rs` cfg gates

Change the module swap from "macOS vs stub" to "macOS vs Windows vs stub":

```rust
#[cfg(target_os = "macos")]
mod clipboard;
#[cfg(target_os = "windows")]
#[path = "clipboard_windows.rs"]
mod clipboard;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "clipboard_stub.rs"]
mod clipboard;
```

**File:** `src-tauri/src/lib.rs` (lines 12-16)

---

## Step 4: Add Windows `list_installed_apps` in `lib.rs`

New `#[cfg(target_os = "windows")]` block for `list_installed_apps`:

- Read from 3 registry paths via `winreg`:
  - `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
  - `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
  - `HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall` (32-bit apps)
- For each subkey: read `DisplayName` → `name`
- Derive `bundle_id` from `DisplayIcon` or `InstallLocation` exe path (extract filename like `msedge.exe`)
- **Aggressive filtering:** skip entries where:
  - `SystemComponent` = 1
  - No `DisplayName`
  - No `DisplayIcon` and no `InstallLocation` (no way to get exe name)
  - Name starts with `KB` (Windows updates)
  - Name contains "Redistributable", "SDK", "Runtime"
- Sort by name, dedup by bundle_id

**File:** `src-tauri/src/lib.rs`

---

## Step 5: Update `capabilities/default.json` if needed

Check if any Tauri permissions are needed for clipboard access on Windows. The macOS implementation uses raw FFI so no Tauri plugin permissions are needed — Windows should be the same since we're using Win32 APIs directly.

**File:** `src-tauri/capabilities/default.json` (verify only)

---

## Step 6: App picker fallback for Windows rules

- Keep manual typed app-id entry enabled even when registry app list is non-empty
- On Windows, manual app-id must be normalized `.exe` filename (lowercase), not path
- Enter key submits exact match from list first, else manual entry

**Files:** `src/App.tsx`, `src/App.css`

---

## Files to modify

| File | Action |
|------|--------|
| `src-tauri/Cargo.toml` | Add `[target.'cfg(target_os = "windows")'.dependencies]` |
| `src-tauri/src/clipboard_windows.rs` | Create (monitor + blocker + types) |
| `src-tauri/src/lib.rs` | Edit cfg gates + add Windows app listing |
| `src/App.tsx` | Allow manual app-id entry even with populated list |
| `src/App.css` | Styles for manual entry + validation hint |

---

## Verification

1. `cargo check` on macOS — confirm no regressions
2. Push to branch — CI builds on all platforms
3. On Windows: open app, copy text between apps, verify "Last Clipboard Source" updates
4. On Windows: add a rule with Browse, verify apps appear from registry
5. On Windows: set block rule, verify Ctrl+V blocked in target app

---

## Decisions made

- **App ID**: exe name (e.g. `msedge.exe`) — natural match between foreground detection and registry listing
- **Registry filter**: aggressive — skip system components, updates, runtimes
- **Paste blocking**: yes, full implementation with low-level keyboard hook
- **Hook lifecycle**: install once, gate behavior with atomic block flag
- **Manual rule entry**: always available; Windows requires `.exe` app-id
