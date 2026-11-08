use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::engine::{Engine, Snapshot};

type SharedEngine = Arc<Mutex<Engine>>;

fn emit_snapshot(app: &AppHandle, snapshot: &Snapshot) {
    if let Err(err) = app.emit("engine://snapshot", snapshot) {
        eprintln!("failed to emit snapshot: {err}");
    }
}

fn run_command(
    state: &State<'_, SharedEngine>,
    app: &AppHandle,
    action: impl FnOnce(&mut Engine, Instant),
) -> Result<Snapshot, String> {
    let now = Instant::now();
    let snapshot = {
        let mut engine = state
            .lock()
            .map_err(|_| "engine lock poisoned".to_string())?;
        action(&mut engine, now);
        engine.snapshot(now)
    };
    emit_snapshot(app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn start(state: State<'_, SharedEngine>, app: AppHandle) -> Result<Snapshot, String> {
    run_command(&state, &app, |engine, now| engine.start(now))
}

#[tauri::command]
pub fn pause(state: State<'_, SharedEngine>, app: AppHandle) -> Result<Snapshot, String> {
    run_command(&state, &app, |engine, now| engine.pause(now))
}

#[tauri::command]
pub fn resume(state: State<'_, SharedEngine>, app: AppHandle) -> Result<Snapshot, String> {
    run_command(&state, &app, |engine, now| engine.resume(now))
}

#[tauri::command]
pub fn stop(state: State<'_, SharedEngine>, app: AppHandle) -> Result<Snapshot, String> {
    run_command(&state, &app, |engine, _| engine.stop())
}

#[tauri::command]
pub fn skip(state: State<'_, SharedEngine>, app: AppHandle) -> Result<Snapshot, String> {
    run_command(&state, &app, |engine, _| engine.skip())
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, SharedEngine>, app: AppHandle) -> Result<Snapshot, String> {
    run_command(&state, &app, |_, _| {})
}

#[tauri::command]
pub fn save_position(app: AppHandle) -> Result<(), String> {
    thread::Builder::new()
        .name("save-position".to_string())
        .spawn(move || {
            thread::sleep(Duration::from_millis(500));
            match app.get_webview_window("pill") {
                Some(window) => {
                    if let Err(err) = crate::win32::save_position(&window) {
                        eprintln!("failed to save window position: {err}");
                    }
                }
                None => eprintln!("save_position: pill window not found"),
            }
        })
        .map(|_handle| ())
        .map_err(|err| format!("failed to start position saver: {err}"))
}
