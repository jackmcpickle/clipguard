use std::ffi::c_void;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use objc2_app_kit::{NSPasteboard, NSWorkspace};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::rules::{self, BlockRule, RuleAction};

const POLL_INTERVAL_MS: u64 = 300;

// --- CGEventTap FFI ---

type CGEventRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFStringRef = *const c_void;

type CGEventTapCallBack = extern "C" fn(
    proxy: *mut c_void,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

const CG_SESSION_EVENT_TAP: u32 = 1;
const CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;
const CG_EVENT_KEY_DOWN_MASK: u64 = 1 << 10;
const CG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;
const CG_KEYBOARD_EVENT_KEYCODE_FIELD: u32 = 9;
const V_KEYCODE: i64 = 9;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CFMachPortInvalidate(tap: CFMachPortRef);
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: CFMachPortRef,
        order: i64,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRemoveSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRunInMode(
        mode: CFStringRef,
        seconds: f64,
        return_after_source_handled: bool,
    ) -> i32;
    static kCFRunLoopDefaultMode: CFStringRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

// --- Tap callback: suppress Cmd+V / Cmd+Shift+V ---

extern "C" fn tap_callback(
    _proxy: *mut c_void,
    _event_type: u32,
    event: CGEventRef,
    _user_info: *mut c_void,
) -> CGEventRef {
    unsafe {
        let flags = CGEventGetFlags(event);
        let keycode = CGEventGetIntegerValueField(event, CG_KEYBOARD_EVENT_KEYCODE_FIELD);
        if keycode == V_KEYCODE && (flags & CG_EVENT_FLAG_MASK_COMMAND) != 0 {
            return std::ptr::null_mut();
        }
    }
    event
}

// --- Blocker thread ---

enum BlockerMsg {
    Enable,
    Disable,
}

fn run_blocker_thread(rx: mpsc::Receiver<BlockerMsg>) {
    let mut active: Option<(CFMachPortRef, CFRunLoopSourceRef)> = None;

    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(BlockerMsg::Enable) => {
                if active.is_some() {
                    continue;
                }
                unsafe {
                    let port = CGEventTapCreate(
                        CG_SESSION_EVENT_TAP,
                        CG_HEAD_INSERT_EVENT_TAP,
                        CG_EVENT_TAP_OPTION_DEFAULT,
                        CG_EVENT_KEY_DOWN_MASK,
                        tap_callback,
                        std::ptr::null_mut(),
                    );
                    if port.is_null() {
                        continue;
                    }
                    let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
                    if source.is_null() {
                        CFMachPortInvalidate(port);
                        CFRelease(port as *const c_void);
                        continue;
                    }
                    let rl = CFRunLoopGetCurrent();
                    CFRunLoopAddSource(rl, source, kCFRunLoopDefaultMode);
                    CGEventTapEnable(port, true);
                    active = Some((port, source));
                }
            }
            Ok(BlockerMsg::Disable) => {
                teardown_tap(&mut active);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                teardown_tap(&mut active);
                break;
            }
        }

        // Process tap events via CFRunLoop
        if active.is_some() {
            unsafe {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, false);
            }
        }
    }
}

fn teardown_tap(active: &mut Option<(CFMachPortRef, CFRunLoopSourceRef)>) {
    if let Some((port, source)) = active.take() {
        unsafe {
            CGEventTapEnable(port, false);
            let rl = CFRunLoopGetCurrent();
            CFRunLoopRemoveSource(rl, source, kCFRunLoopDefaultMode);
            CFMachPortInvalidate(port);
            CFRelease(source as *const c_void);
            CFRelease(port as *const c_void);
        }
    }
}

// --- Types ---

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

// NOTE: These AppKit calls are made from a background thread. Apple docs say AppKit
// should be main-thread-only, but NSPasteboard.changeCount and NSRunningApplication
// properties are atomic/read-only and widely used off-main in practice (e.g. clipboard-master).
// A future improvement could dispatch to the main queue for full correctness.
fn get_frontmost_app() -> (Option<String>, Option<String>) {
    let workspace = NSWorkspace::sharedWorkspace();
    if let Some(app) = workspace.frontmostApplication() {
        let bundle_id = app.bundleIdentifier().map(|s| s.to_string());
        let name = app.localizedName().map(|s| s.to_string());
        (bundle_id, name)
    } else {
        (None, None)
    }
}

fn get_pasteboard_change_count() -> isize {
    let pb = NSPasteboard::generalPasteboard();
    pb.changeCount()
}

fn is_cross_app(source: &ClipboardEvent, dest_bundle_id: &str) -> bool {
    match &source.source_app_id {
        Some(src_id) => !src_id.eq_ignore_ascii_case(dest_bundle_id),
        None => true,
    }
}

pub fn start_clipboard_monitor(app: AppHandle, state: Arc<Mutex<ClipboardState>>) {
    // Spawn blocker thread with its own CFRunLoop
    let (blocker_tx, blocker_rx) = mpsc::channel();
    thread::spawn(|| run_blocker_thread(blocker_rx));

    // Spawn monitor thread
    thread::spawn(move || {
        let mut last_change_count = get_pasteboard_change_count();
        let mut last_frontmost_id: Option<String> = None;
        let mut last_warned: Option<(Option<String>, Option<String>)> = None;
        let mut block_active = false;

        loop {
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

            let (current_id, current_name) = get_frontmost_app();

            // Detect clipboard changes (always track, even when disabled)
            let current_count = get_pasteboard_change_count();
            if current_count != last_change_count {
                last_change_count = current_count;
                last_warned = None;

                // New clipboard content — disable active block, re-evaluate on next switch
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

            // Switched away from blocked app — disable tap
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

            let source = state
                .lock()
                .ok()
                .and_then(|s| s.last_copy_source.clone());

            let Some(source) = source else {
                continue;
            };

            // Same-app paste always allowed
            if !is_cross_app(&source, dest_id) {
                continue;
            }

            // Check rules
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

            // Deduplicate: skip if we already warned for this exact (src, dst) pair
            let warn_key = (source.source_app_id.clone(), current_id.clone());
            if last_warned.as_ref() == Some(&warn_key) {
                continue;
            }
            last_warned = Some(warn_key);

            let src_name = source
                .source_app_name
                .as_deref()
                .unwrap_or("Unknown app");
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
                    let ax_trusted = unsafe { AXIsProcessTrusted() };
                    if ax_trusted {
                        let _ = blocker_tx.send(BlockerMsg::Enable);
                        block_active = true;
                        if let Ok(mut s) = state.lock() {
                            s.blocking_active = true;
                        }
                        (
                            format!("Paste blocked: {} → {}", src_name, dst_name),
                            true,
                        )
                    } else {
                        // Fall back to notify when accessibility not granted
                        (
                            format!(
                                "Clipboard from {}. Pasting into {} would be blocked (grant Accessibility).",
                                src_name, dst_name
                            ),
                            false,
                        )
                    }
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
