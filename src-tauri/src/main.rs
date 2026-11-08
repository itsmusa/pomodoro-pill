#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod commands;
mod engine;
mod runtime;
mod settings;
mod sys_events;
mod tray;
mod win32;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::Manager;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use engine::{Config, Engine};
use runtime::RuntimeSignal;

fn main() {
    let loaded = settings::load();
    let config = Config {
        focus: Duration::from_secs(u64::from(loaded.focus_min) * 60),
        brk: Duration::from_secs(u64::from(loaded.break_min) * 60),
        idle_reset: Duration::from_secs(u64::from(loaded.idle_reset_min) * 60),
    };

    let engine = Arc::new(Mutex::new(Engine::new(config)));
    let tick_engine = Arc::clone(&engine);
    let shared_settings = Arc::new(Mutex::new(loaded));
    let (signal_tx, signal_rx) = mpsc::channel();
    let hotkey_tx = signal_tx.clone();
    let audio_tx = audio::spawn(Arc::clone(&shared_settings));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("pill") {
                if let Err(err) = window.show() {
                    eprintln!("failed to re-show pill: {err}");
                }
            }
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |_app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let _ = hotkey_tx.send(RuntimeSignal::Toggle);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(engine)
        .setup(move |app| {
            runtime::spawn_tick_loop(app.handle().clone(), tick_engine, signal_rx, audio_tx);
            sys_events::spawn(signal_tx);

            if let Some(window) = app.get_webview_window("pill") {
                if let Err(err) = win32::setup(&window) {
                    eprintln!("window setup failed: {err}");
                }
                if let Err(err) = window.show() {
                    eprintln!("failed to show pill window: {err}");
                }
                if let Err(err) = win32::apply_styles(&window) {
                    eprintln!("failed to re-apply window styles: {err}");
                }
            }

            win32::spawn_fullscreen_watcher(app.handle().clone(), Arc::clone(&shared_settings));

            if let Err(err) = tray::setup(app.handle(), Arc::clone(&shared_settings)) {
                eprintln!("tray setup failed: {err}");
            }

            let autostart_enabled = match shared_settings.lock() {
                Ok(settings) => settings.autostart,
                Err(poisoned) => poisoned.into_inner().autostart,
            };
            let result = if autostart_enabled {
                app.autolaunch().enable()
            } else {
                app.autolaunch().disable()
            };
            if let Err(err) = result {
                eprintln!("failed to sync autostart: {err}");
            }

            if let Err(err) = app.global_shortcut().register("Ctrl+Alt+P") {
                eprintln!("failed to register Ctrl+Alt+P: {err}");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start,
            commands::pause,
            commands::resume,
            commands::stop,
            commands::skip,
            commands::get_snapshot,
            commands::save_position,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Pomodoro Pill");

    app.run(|_app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            sys_events::shutdown();
        }
    });
}
