#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

#[cfg(target_os = "macos")]
mod clipboard;
#[cfg(not(target_os = "macos"))]
#[path = "clipboard_stub.rs"]
mod clipboard;
mod config;
mod rules;

use clipboard::ClipboardState;
use rules::BlockRule;

struct ToggleMenuItem(tauri::menu::MenuItem<tauri::Wry>);

#[derive(Debug, Clone, Serialize)]
struct AppBundleInfo {
    bundle_id: String,
    name: String,
}

#[tauri::command]
fn get_clipboard_source(
    state: tauri::State<'_, Arc<Mutex<ClipboardState>>>,
) -> Option<clipboard::ClipboardEvent> {
    state.lock().ok().and_then(|s| s.last_copy_source.clone())
}

#[tauri::command]
fn get_enabled(state: tauri::State<'_, Arc<Mutex<ClipboardState>>>) -> bool {
    state.lock().ok().map(|s| s.enabled).unwrap_or(true)
}

#[tauri::command]
fn set_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<ClipboardState>>>,
    toggle: tauri::State<'_, ToggleMenuItem>,
    enabled: bool,
) {
    if let Ok(mut s) = state.lock() {
        s.enabled = enabled;
    }
    let label = if enabled {
        "Disable Guard"
    } else {
        "Enable Guard"
    };
    let _ = toggle.0.set_text(label);
    let _ = app.emit("guard-toggled", enabled);
}

#[tauri::command]
fn get_rules(state: tauri::State<'_, Arc<Mutex<ClipboardState>>>) -> Vec<BlockRule> {
    state
        .lock()
        .ok()
        .map(|s| s.rules.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn set_rules(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<ClipboardState>>>,
    new_rules: Vec<BlockRule>,
) -> Result<(), String> {
    rules::save(&app, &new_rules)?;
    if let Ok(mut s) = state.lock() {
        s.rules = new_rules;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn read_app_bundle_info(path: &Path) -> Option<(String, String)> {
    let plist_path = path.join("Contents/Info.plist");
    let val = plist::Value::from_file(&plist_path).ok()?;
    let dict = val.as_dictionary()?;
    let bundle_id = dict.get("CFBundleIdentifier")?.as_string()?.to_string();
    let name = dict
        .get("CFBundleDisplayName")
        .or(dict.get("CFBundleName"))
        .and_then(|v| v.as_string())
        .unwrap_or("Unknown")
        .to_string();
    Some((bundle_id, name))
}

#[tauri::command]
fn list_apps() -> Vec<AppBundleInfo> {
    list_installed_apps()
}

#[cfg(target_os = "macos")]
fn list_installed_apps() -> Vec<AppBundleInfo> {
    let dirs = [
        PathBuf::from("/Applications"),
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join("Applications"))
            .unwrap_or_default(),
    ];
    let mut apps = Vec::new();
    for dir in &dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "app") {
                    if let Some((bundle_id, name)) = read_app_bundle_info(&path) {
                        apps.push(AppBundleInfo { bundle_id, name });
                    }
                }
            }
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.bundle_id == b.bundle_id);
    apps
}

#[cfg(not(target_os = "macos"))]
fn list_installed_apps() -> Vec<AppBundleInfo> {
    Vec::new()
}

#[tauri::command]
fn check_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        // AXIsProcessTrusted from ApplicationServices framework
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }
        unsafe { AXIsProcessTrusted() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

#[tauri::command]
fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let _ = Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            get_clipboard_source,
            get_enabled,
            set_enabled,
            get_rules,
            set_rules,
            list_apps,
            check_accessibility,
            open_accessibility_settings,
        ])
        .setup(|app| {
            // Hide dock icon — tray-only app
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Auto-enable launch at login on first run
            let mut app_config = config::load(&app.handle());
            if !app_config.autostart_initialized {
                use tauri_plugin_autostart::ManagerExt;
                let _ = app.autolaunch().enable();
                app_config.autostart_initialized = true;
                let _ = config::save(&app.handle(), &app_config);
            }

            // Load rules
            let loaded_rules = rules::load(&app.handle());

            // Clipboard state — shared between tray menu and monitor thread
            let clip_state = Arc::new(Mutex::new(ClipboardState {
                last_copy_source: None,
                enabled: true,
                rules: loaded_rules,
                blocking_active: false,
            }));

            // Build tray menu
            let toggle_item = MenuItemBuilder::with_id("toggle", "Disable Guard").build(app)?;
            let show_item = MenuItemBuilder::with_id("show", "Settings...").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = Menu::with_items(app, &[&toggle_item, &show_item, &quit_item])?;

            // Build tray icon
            let icon = app.default_window_icon().cloned().unwrap_or_else(|| {
                tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
                    .expect("bundled icon")
            });

            app.manage(ToggleMenuItem(toggle_item.clone()));

            let state_for_tray = clip_state.clone();
            let tray = TrayIconBuilder::new()
                .icon(icon)
                .tooltip("Clipboard Guard")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "toggle" => {
                        if let Ok(mut s) = state_for_tray.lock() {
                            s.enabled = !s.enabled;
                            let label = if s.enabled {
                                "Disable Guard"
                            } else {
                                "Enable Guard"
                            };
                            let toggle = app.state::<ToggleMenuItem>();
                            let _ = toggle.0.set_text(label);
                            let _ = app.emit("guard-toggled", s.enabled);
                        }
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            #[cfg(target_os = "macos")]
                            let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            app.manage(tray);
            app.manage(clip_state.clone());
            clipboard::start_clipboard_monitor(app.handle().clone(), clip_state);

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                let _ = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
