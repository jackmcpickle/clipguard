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
#[cfg(target_os = "windows")]
#[path = "clipboard_windows.rs"]
mod clipboard;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

#[tauri::command]
fn is_windows_platform() -> bool {
    cfg!(target_os = "windows")
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

#[cfg(target_os = "windows")]
fn list_installed_apps() -> Vec<AppBundleInfo> {
    use winreg::enums::*;
    use winreg::RegKey;

    fn normalized_windows_app_id(raw: &str) -> Option<String> {
        let stripped = raw
            .split(',')
            .next()
            .unwrap_or(raw)
            .trim()
            .trim_matches('"')
            .trim();
        let file = stripped
            .rsplit(|c| c == '\\' || c == '/')
            .next()
            .unwrap_or(stripped)
            .trim();
        if file.is_empty() {
            return None;
        }
        let lower = file.to_ascii_lowercase();
        if !lower.ends_with(".exe") {
            return None;
        }
        Some(lower)
    }

    let paths = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ];

    let mut apps = Vec::new();

    for (root, path) in &paths {
        let Ok(key) = RegKey::predef(*root).open_subkey_with_flags(path, KEY_READ) else {
            continue;
        };
        for name in key.enum_keys().filter_map(|k| k.ok()) {
            let Ok(subkey) = key.open_subkey_with_flags(&name, KEY_READ) else {
                continue;
            };

            // Skip system components
            if subkey.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 {
                continue;
            }

            let Ok(display_name) = subkey.get_value::<String, _>("DisplayName") else {
                continue;
            };
            let display_name = display_name.trim().to_string();
            if display_name.is_empty() {
                continue;
            }
            let display_name_lc = display_name.to_ascii_lowercase();

            // Skip Windows updates, runtimes, SDKs
            if display_name_lc.starts_with("kb")
                || display_name_lc.contains("redistributable")
                || display_name_lc.contains("sdk")
                || display_name_lc.contains("runtime")
            {
                continue;
            }

            // Derive exe name from DisplayIcon or InstallLocation
            let exe_name = subkey
                .get_value::<String, _>("DisplayIcon")
                .ok()
                .and_then(|icon| normalized_windows_app_id(&icon))
                .or_else(|| {
                    subkey
                        .get_value::<String, _>("InstallLocation")
                        .ok()
                        .and_then(|loc| {
                            // Try to find an exe in the install location
                            std::fs::read_dir(&loc).ok().and_then(|entries| {
                                entries
                                    .filter_map(|e| e.ok())
                                    .find(|e| {
                                        e.path()
                                            .extension()
                                            .map_or(false, |ext| ext.eq_ignore_ascii_case("exe"))
                                    })
                                    .and_then(|e| {
                                        normalized_windows_app_id(
                                            e.file_name().to_string_lossy().as_ref(),
                                        )
                                    })
                            })
                        })
                });

            let Some(bundle_id) = exe_name else {
                continue;
            };

            apps.push(AppBundleInfo {
                bundle_id,
                name: display_name,
            });
        }
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.bundle_id.eq_ignore_ascii_case(&b.bundle_id));
    apps
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
            is_windows_platform,
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
