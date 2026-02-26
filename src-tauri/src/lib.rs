use std::sync::{Arc, Mutex};

use tauri::{
    menu::{Menu, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

#[cfg(target_os = "macos")]
mod clipboard;

#[tauri::command]
fn get_clipboard_source(
    state: tauri::State<'_, Arc<Mutex<clipboard::ClipboardState>>>,
) -> Option<clipboard::ClipboardEvent> {
    state.lock().ok().and_then(|s| s.last_copy_source.clone())
}

#[tauri::command]
fn get_enabled(state: tauri::State<'_, Arc<Mutex<clipboard::ClipboardState>>>) -> bool {
    state.lock().ok().map(|s| s.enabled).unwrap_or(true)
}

#[tauri::command]
fn set_enabled(state: tauri::State<'_, Arc<Mutex<clipboard::ClipboardState>>>, enabled: bool) {
    if let Ok(mut s) = state.lock() {
        s.enabled = enabled;
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
        ])
        .setup(|app| {
            // Hide dock icon — tray-only app
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Clipboard state — shared between tray menu and monitor thread
            let clip_state = Arc::new(Mutex::new(clipboard::ClipboardState {
                last_copy_source: None,
                enabled: true,
            }));

            // Build tray menu
            let toggle_item = MenuItemBuilder::with_id("toggle", "Disable Guard")
                .build(app)?;
            let show_item = MenuItemBuilder::with_id("show", "Settings...").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = Menu::with_items(app, &[&toggle_item, &show_item, &quit_item])?;

            // Build tray icon
            let icon = app
                .default_window_icon()
                .cloned()
                .unwrap_or_else(|| {
                    tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
                        .expect("bundled icon")
                });

            // Capture MenuItem handle for toggling text
            let toggle_handle = toggle_item.clone();
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
                            let _ = toggle_handle.set_text(label);
                            let _ = app.emit("guard-toggled", s.enabled);
                        }
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
