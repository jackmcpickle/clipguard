use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::rules::BlockRule;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEvent {
    pub source_app_id: Option<String>,
    pub source_app_name: Option<String>,
}

pub struct ClipboardState {
    pub last_copy_source: Option<ClipboardEvent>,
    pub enabled: bool,
    pub rules: Vec<BlockRule>,
    pub blocking_active: bool,
}

pub fn start_clipboard_monitor(_app: AppHandle, _state: Arc<Mutex<ClipboardState>>) {
    // Clipboard monitoring not implemented for this platform
}
