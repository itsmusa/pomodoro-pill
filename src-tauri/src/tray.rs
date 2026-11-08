use std::process::Command;
use std::sync::{Arc, Mutex};

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::settings::{self, Settings};

pub fn setup(app: &AppHandle, settings: Arc<Mutex<Settings>>) -> Result<(), String> {
    let current = settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .clone();

    let sound = CheckMenuItemBuilder::with_id("sound", "Sound")
        .checked(current.sound)
        .build(app)
        .map_err(|err| err.to_string())?;
    let autostart = CheckMenuItemBuilder::with_id("autostart", "Start with Windows")
        .checked(current.autostart)
        .build(app)
        .map_err(|err| err.to_string())?;
    let hide = CheckMenuItemBuilder::with_id("hide_fullscreen", "Hide in fullscreen")
        .checked(current.hide_in_fullscreen)
        .build(app)
        .map_err(|err| err.to_string())?;

    let reset = MenuItemBuilder::with_id("reset_position", "Reset position")
        .build(app)
        .map_err(|err| err.to_string())?;
    let open = MenuItemBuilder::with_id("open_settings", "Open settings file")
        .build(app)
        .map_err(|err| err.to_string())?;
    let quit = MenuItemBuilder::with_id("quit", "Quit")
        .build(app)
        .map_err(|err| err.to_string())?;

    let menu = MenuBuilder::new(app)
        .item(&sound)
        .item(&autostart)
        .item(&hide)
        .separator()
        .item(&reset)
        .item(&open)
        .separator()
        .item(&quit)
        .build()
        .map_err(|err| err.to_string())?;

    let icon =
        Image::from_bytes(include_bytes!("../icons/tray.png")).map_err(|err| err.to_string())?;

    let settings_for_menu = Arc::clone(&settings);
    let sound_for_menu = sound.clone();
    let autostart_for_menu = autostart.clone();
    let hide_for_menu = hide.clone();

    let tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip("Pomodoro Pill")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            handle_menu(
                app,
                event.id().0.as_str(),
                &settings_for_menu,
                &sound_for_menu,
                &autostart_for_menu,
                &hide_for_menu,
            );
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|err| err.to_string())?;

    // Tauri removes the tray icon when the last handle is dropped.
    app.manage(tray);
    Ok(())
}

fn handle_menu(
    app: &AppHandle,
    id: &str,
    settings: &Arc<Mutex<Settings>>,
    sound: &CheckMenuItem<Wry>,
    autostart: &CheckMenuItem<Wry>,
    hide: &CheckMenuItem<Wry>,
) {
    match id {
        "sound" => {
            let value = update_setting(settings, |settings| {
                settings.sound = !settings.sound;
                settings.sound
            });
            if let Err(err) = sound.set_checked(value) {
                eprintln!("failed to update Sound check: {err}");
            }
        }
        "hide_fullscreen" => {
            let value = update_setting(settings, |settings| {
                settings.hide_in_fullscreen = !settings.hide_in_fullscreen;
                settings.hide_in_fullscreen
            });
            if let Err(err) = hide.set_checked(value) {
                eprintln!("failed to update Hide in fullscreen check: {err}");
            }
        }
        "autostart" => {
            let value = update_setting(settings, |settings| {
                settings.autostart = !settings.autostart;
                settings.autostart
            });

            let result = if value {
                app.autolaunch().enable()
            } else {
                app.autolaunch().disable()
            };

            if let Err(err) = result {
                eprintln!("failed to toggle autostart: {err}");
                let reverted = update_setting(settings, |settings| {
                    settings.autostart = !value;
                    settings.autostart
                });
                if let Err(err) = autostart.set_checked(reverted) {
                    eprintln!("failed to revert autostart check: {err}");
                }
            } else if let Err(err) = autostart.set_checked(value) {
                eprintln!("failed to update autostart check: {err}");
            }
        }
        "reset_position" => {
            if let Err(err) = settings::clear_position() {
                eprintln!("failed to clear saved position: {err}");
            }
            if let Some(window) = app.get_webview_window("pill") {
                if let Err(err) = crate::win32::reset_position(&window) {
                    eprintln!("failed to reset window position: {err}");
                }
            }
        }
        "open_settings" => open_settings(),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn update_setting(
    settings: &Arc<Mutex<Settings>>,
    change: impl FnOnce(&mut Settings) -> bool,
) -> bool {
    let (value, snapshot) = {
        let mut guard = match settings.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let value = change(&mut guard);
        (value, guard.clone())
    };

    if let Err(err) = settings::save(&snapshot) {
        eprintln!("failed to save settings: {err}");
    }

    value
}

fn open_settings() {
    match settings::ensure_file() {
        Ok(path) => {
            if let Err(err) = Command::new("notepad.exe").arg(&path).spawn() {
                eprintln!("failed to open settings file: {err}");
            }
        }
        Err(err) => eprintln!("failed to create settings file: {err}"),
    }
}

fn toggle_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("pill") else {
        return;
    };

    match window.is_visible() {
        Ok(true) => {
            if let Err(err) = window.hide() {
                eprintln!("failed to hide pill: {err}");
            }
        }
        Ok(false) => {
            if let Err(err) = window.show() {
                eprintln!("failed to show pill: {err}");
            }
        }
        Err(err) => eprintln!("failed to read pill visibility: {err}"),
    }
}
