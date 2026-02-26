use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use objc2_app_kit::{NSPasteboard, NSWorkspace};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

const POLL_INTERVAL_MS: u64 = 300;

/// Bundle IDs of apps where pasting untrusted content is dangerous
const MONITORED_DESTINATIONS: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "io.alacritty",
    "com.github.wez.wezterm",
    "net.kovidgoyal.kitty",
    "co.zeit.hyper",
    "com.mitchellh.ghostty",
    "com.raphaelamorim.rio",
];

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
}

pub struct ClipboardState {
    pub last_copy_source: Option<ClipboardEvent>,
    pub enabled: bool,
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

fn is_monitored_destination(bundle_id: &str) -> bool {
    MONITORED_DESTINATIONS
        .iter()
        .any(|&id| id.eq_ignore_ascii_case(bundle_id))
}

fn is_cross_app(source: &ClipboardEvent, dest_bundle_id: &str) -> bool {
    match &source.source_app_id {
        Some(src_id) => !src_id.eq_ignore_ascii_case(dest_bundle_id),
        None => true,
    }
}

pub fn start_clipboard_monitor(app: AppHandle, state: Arc<Mutex<ClipboardState>>) {
    thread::spawn(move || {
        let mut last_change_count = get_pasteboard_change_count();
        let mut last_frontmost_id: Option<String> = None;
        // Track last warned (source_app_id, dest_app_id) to prevent spam
        let mut last_warned: Option<(Option<String>, Option<String>)> = None;

        loop {
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

            // Single frontmost app call per iteration to avoid race conditions
            let (current_id, current_name) = get_frontmost_app();

            // Detect clipboard changes (always track, even when disabled)
            let current_count = get_pasteboard_change_count();
            if current_count != last_change_count {
                last_change_count = current_count;
                // Reset warning dedup on new clipboard content
                last_warned = None;

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
                last_frontmost_id = current_id;
                continue;
            }

            // Detect app switches
            let switched = current_id != last_frontmost_id;
            last_frontmost_id = current_id.clone();

            if !switched {
                continue;
            }

            let Some(dest_id) = &current_id else {
                continue;
            };

            if !is_monitored_destination(dest_id) {
                continue;
            }

            let source = state
                .lock()
                .ok()
                .and_then(|s| s.last_copy_source.clone());

            let Some(source) = source else {
                continue;
            };

            if !is_cross_app(&source, dest_id) {
                continue;
            }

            // Deduplicate: skip if we already warned for this exact (src, dst) pair
            let warn_key = (source.source_app_id.clone(), current_id.clone());
            if last_warned.as_ref() == Some(&warn_key) {
                continue;
            }
            last_warned = Some(warn_key);

            let warning = PasteWarning {
                source_app_id: source.source_app_id.clone(),
                source_app_name: source.source_app_name.clone(),
                dest_app_id: current_id.clone(),
                dest_app_name: current_name.clone(),
            };

            let src_name = source
                .source_app_name
                .as_deref()
                .unwrap_or("Unknown app");
            let dst_name = current_name.as_deref().unwrap_or("Terminal");

            let _ = app
                .notification()
                .builder()
                .title("Clipboard Guard")
                .body(format!(
                    "Clipboard was copied in {}. Be careful pasting into {}.",
                    src_name, dst_name
                ))
                .show();

            let _ = app.emit("paste-warning", &warning);
        }
    });
}
