use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use windows::core::PWSTR;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Foundation::{CloseHandle, LRESULT};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::rules::{self, BlockRule, RuleAction};

const POLL_INTERVAL_MS: u64 = 300;
const VK_V: u32 = 0x56;

/// Global flag read by the keyboard hook callback to decide whether to suppress Ctrl+V.
static BLOCK_PASTE: AtomicBool = AtomicBool::new(false);

// --- Types (same public API as clipboard.rs) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEvent {
    pub source_app_id: Option<String>,
    pub source_app_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasteWarning {
    pub source_app_id: Option<String>,
    pub source_app_name: Option<String>,
    pub dest_app_id: Option<String>,
    pub dest_app_name: Option<String>,
    pub blocked: bool,
}

pub struct ClipboardState {
    pub last_copy_source: Option<ClipboardEvent>,
    pub enabled: bool,
    pub rules: Vec<BlockRule>,
    pub blocking_active: bool,
}

// --- Foreground app detection ---

/// Returns (exe_filename, exe_stem) of the foreground window's process.
/// exe_filename (e.g. "msedge.exe") is used as the app id.
fn get_frontmost_app() -> (Option<String>, Option<String>) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return (None, None);
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return (None, None);
        }

        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return (None, None);
        };

        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);

        if ok.is_err() || size == 0 {
            return (None, None);
        }

        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let filename = path.rsplit('\\').next().unwrap_or(&path).to_string();
        let app_id = filename.to_ascii_lowercase();
        let stem = filename
            .strip_suffix(".exe")
            .or_else(|| filename.strip_suffix(".EXE"))
            .unwrap_or(&filename)
            .to_string();

        (Some(app_id), Some(stem))
    }
}

// --- Clipboard sequence number ---

fn get_clipboard_sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

// --- Cross-app check ---

fn is_cross_app(source: &ClipboardEvent, dest_app_id: &str) -> bool {
    match &source.source_app_id {
        Some(src_id) => !src_id.eq_ignore_ascii_case(dest_app_id),
        None => true,
    }
}

// --- Low-level keyboard hook callback ---

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && BLOCK_PASTE.load(Ordering::Relaxed) {
        let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let is_keydown = wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize;
        if is_keydown && info.vkCode == VK_V {
            let ctrl = GetAsyncKeyState(VK_CONTROL.0 as i32);
            if ctrl < 0 {
                // Suppress the keystroke
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

// --- Blocker thread ---

enum BlockerMsg {
    Enable,
    Disable,
}

fn run_blocker_thread(rx: mpsc::Receiver<BlockerMsg>) {
    unsafe {
        let hmodule = GetModuleHandleW(None).ok();
        let hinstance = hmodule.map(|m| HINSTANCE(m.0));
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), hinstance, 0);
        let Ok(hook) = hook else {
            eprintln!("clipboard_windows: failed to install keyboard hook");
            return;
        };

        // Message pump — required for low-level hooks to work.
        // We check for blocker messages between iterations.
        let mut msg = MSG::default();
        loop {
            // Process any pending blocker commands
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    BlockerMsg::Enable => BLOCK_PASTE.store(true, Ordering::Relaxed),
                    BlockerMsg::Disable => BLOCK_PASTE.store(false, Ordering::Relaxed),
                }
            }

            // Pump one message (timeout ~50ms via MsgWaitForMultipleObjects is complex;
            // PeekMessageW with PM_REMOVE is simpler but busy-loops. GetMessageW blocks,
            // which is fine — the hook still fires because Windows dispatches hook calls
            // into this thread's message queue.)
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 <= 0 {
                break; // WM_QUIT or error
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        BLOCK_PASTE.store(false, Ordering::Relaxed);
        let _ = UnhookWindowsHookEx(hook);
    }
}

// --- Monitor thread + public entry point ---

pub fn start_clipboard_monitor(app: AppHandle, state: Arc<Mutex<ClipboardState>>) {
    // Spawn blocker thread (owns the keyboard hook + message pump)
    let (blocker_tx, blocker_rx) = mpsc::channel();
    thread::spawn(|| run_blocker_thread(blocker_rx));

    // Spawn monitor thread
    thread::spawn(move || {
        let mut last_seq = get_clipboard_sequence();
        let mut last_frontmost_id: Option<String> = None;
        let mut last_warned: Option<(Option<String>, Option<String>)> = None;
        let mut block_active = false;

        loop {
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

            let (current_id, current_name) = get_frontmost_app();

            // Detect clipboard changes
            let current_seq = get_clipboard_sequence();
            if current_seq != last_seq {
                last_seq = current_seq;
                last_warned = None;

                if block_active {
                    let _ = blocker_tx.send(BlockerMsg::Disable);
                    block_active = false;
                    if let Ok(mut s) = state.lock() {
                        s.blocking_active = false;
                    }
                }

                let event = ClipboardEvent {
                    source_app_id: current_id.clone(),
                    source_app_name: current_name.clone(),
                };

                if let Ok(mut s) = state.lock() {
                    s.last_copy_source = Some(event.clone());
                }

                let _ = app.emit("clipboard-changed", &event);
            }

            let is_enabled = state.lock().ok().map(|s| s.enabled).unwrap_or(true);
            if !is_enabled {
                if block_active {
                    let _ = blocker_tx.send(BlockerMsg::Disable);
                    block_active = false;
                    if let Ok(mut s) = state.lock() {
                        s.blocking_active = false;
                    }
                }
                last_frontmost_id = current_id;
                continue;
            }

            // Detect app switches
            let switched = current_id != last_frontmost_id;
            last_frontmost_id = current_id.clone();

            if !switched {
                continue;
            }

            // Switched away — disable block
            if block_active {
                let _ = blocker_tx.send(BlockerMsg::Disable);
                block_active = false;
                if let Ok(mut s) = state.lock() {
                    s.blocking_active = false;
                }
            }

            let Some(dest_id) = &current_id else {
                continue;
            };

            let source = state.lock().ok().and_then(|s| s.last_copy_source.clone());

            let Some(source) = source else {
                continue;
            };

            if !is_cross_app(&source, dest_id) {
                continue;
            }

            let current_rules = state
                .lock()
                .ok()
                .map(|s| s.rules.clone())
                .unwrap_or_default();
            let Some(matched) =
                rules::matches_rule(&current_rules, source.source_app_id.as_deref(), dest_id)
            else {
                continue;
            };

            let warn_key = (source.source_app_id.clone(), current_id.clone());
            if last_warned.as_ref() == Some(&warn_key) {
                continue;
            }
            last_warned = Some(warn_key);

            let src_name = source.source_app_name.as_deref().unwrap_or("Unknown app");
            let dst_name = current_name.as_deref().unwrap_or("Unknown app");

            let (body, blocked) = match matched.action {
                RuleAction::Notify => (
                    format!(
                        "Clipboard from {}. Be careful pasting into {}.",
                        src_name, dst_name
                    ),
                    false,
                ),
                RuleAction::Block => {
                    let _ = blocker_tx.send(BlockerMsg::Enable);
                    block_active = true;
                    if let Ok(mut s) = state.lock() {
                        s.blocking_active = true;
                    }
                    (format!("Paste blocked: {} → {}", src_name, dst_name), true)
                }
            };

            let _ = app
                .notification()
                .builder()
                .title("Clipboard Guard")
                .body(body)
                .show();

            let warning = PasteWarning {
                source_app_id: source.source_app_id,
                source_app_name: source.source_app_name,
                dest_app_id: current_id,
                dest_app_name: current_name,
                blocked,
            };

            let _ = app.emit("paste-warning", &warning);
        }
    });
}
