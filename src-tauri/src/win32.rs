#![cfg(windows)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};
use windows::core::Error;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    IVirtualDesktopManager, SHQueryUserNotificationState, VirtualDesktopManager, QUNS_BUSY,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos,
    EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    WINEVENT_OUTOFCONTEXT, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::settings::{self, Settings};

const PILL_LEFT: f64 = 24.0;
const PILL_TOP: f64 = 24.0;
const PILL_MAX_WIDTH: f64 = 236.0;
const PILL_HEIGHT: f64 = 58.0;
const WINDOW_WIDTH: f64 = 284.0;
const DEFAULT_MARGIN: f64 = 16.0;
const TOPMOST_INTERVAL: Duration = Duration::from_secs(5);

static PILL_HWND: AtomicIsize = AtomicIsize::new(0);
static DESKTOP_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static DESKTOP_MANAGER: RefCell<Option<IVirtualDesktopManager>> = const { RefCell::new(None) };
}

struct MonitorWork {
    work: RECT,
    scale: f64,
}

pub fn setup(window: &WebviewWindow) -> Result<(), String> {
    apply_styles(window)?;
    let hwnd = window.hwnd().map_err(|err| err.to_string())?;
    init_desktop_manager();
    install_foreground_hook(hwnd);
    start_topmost_timer(hwnd);
    start_desktop_timer(hwnd);
    restore_position(window, hwnd)
}

/// Tauri re-applies its own extended styles when the window becomes visible.
/// Call this again after `show()` so NOACTIVATE and TOOLWINDOW survive.
pub fn apply_styles(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|err| err.to_string())?;
    apply_styles_to_hwnd(hwnd);
    Ok(())
}

pub fn save_position(window: &WebviewWindow) -> Result<(), String> {
    let position = window.outer_position().map_err(|err| err.to_string())?;
    settings::save_position(settings::Position {
        x: position.x,
        y: position.y,
    })
}

pub fn reset_position(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|err| err.to_string())?;
    let (x, y, _) = default_position(hwnd)?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|err| err.to_string())
}

pub fn spawn_fullscreen_watcher(app: AppHandle, settings: Arc<Mutex<Settings>>) {
    let spawned = thread::Builder::new()
        .name("fullscreen-watch".to_string())
        .spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            let mut hidden = false;
            let mut query_failed = false;

            loop {
                thread::sleep(Duration::from_secs(2));

                let enabled = match settings.lock() {
                    Ok(settings) => settings.hide_in_fullscreen,
                    Err(poisoned) => poisoned.into_inner().hide_in_fullscreen,
                };

                if !enabled {
                    if hidden {
                        if let Some(window) = app.get_webview_window("pill") {
                            if let Err(err) = window.show() {
                                eprintln!("failed to show pill after fullscreen: {err}");
                            }
                        }
                        hidden = false;
                    }
                    continue;
                }

                let fullscreen = match unsafe { SHQueryUserNotificationState() } {
                    Ok(state) => state == QUNS_BUSY || state == QUNS_RUNNING_D3D_FULL_SCREEN,
                    Err(err) => {
                        if !query_failed {
                            eprintln!("failed to query fullscreen state: {err}");
                            query_failed = true;
                        }
                        false
                    }
                };

                if fullscreen && !hidden {
                    if let Some(window) = app.get_webview_window("pill") {
                        match window.hide() {
                            Ok(()) => hidden = true,
                            Err(err) => eprintln!("failed to hide pill in fullscreen: {err}"),
                        }
                    }
                } else if !fullscreen && hidden {
                    if let Some(window) = app.get_webview_window("pill") {
                        if let Err(err) = window.show() {
                            eprintln!("failed to show pill after fullscreen: {err}");
                        }
                    }
                    hidden = false;
                }
            }
        });

    if let Err(err) = spawned {
        eprintln!("failed to start fullscreen watcher: {err}");
    }
}

fn apply_styles_to_hwnd(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = (style | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize))
            & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
    }
}

fn install_foreground_hook(hwnd: HWND) {
    PILL_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };

    if hook.is_invalid() {
        eprintln!("failed to install foreground hook; relying on the 5 s topmost timer");
    }
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    let raw = PILL_HWND.load(Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    let pill = HWND(raw as *mut c_void);
    let _ = SetWindowPos(
        pill,
        Some(HWND_TOPMOST),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );

    DESKTOP_MANAGER.with(|slot| {
        if let Some(manager) = slot.borrow().as_ref() {
            follow_desktop(manager, pill);
        }
    });
}

fn init_desktop_manager() {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    let manager: IVirtualDesktopManager =
        match unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL) } {
            Ok(manager) => manager,
            Err(err) => {
                log_desktop_failure("create manager", &err);
                return;
            }
        };

    DESKTOP_MANAGER.with(|slot| *slot.borrow_mut() = Some(manager));
}

fn start_desktop_timer(hwnd: HWND) {
    let raw = hwnd.0 as isize;
    let spawned = thread::Builder::new()
        .name("virtual-desktop".to_string())
        .spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

            let manager: IVirtualDesktopManager =
                match unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL) } {
                    Ok(manager) => manager,
                    Err(err) => {
                        log_desktop_failure("create manager", &err);
                        return;
                    }
                };

            let pill = HWND(raw as *mut c_void);
            loop {
                thread::sleep(Duration::from_secs(1));
                follow_desktop(&manager, pill);
            }
        });

    if let Err(err) = spawned {
        eprintln!("failed to start virtual desktop timer: {err}");
    }
}

fn follow_desktop(manager: &IVirtualDesktopManager, pill: HWND) {
    unsafe {
        match manager.IsWindowOnCurrentVirtualDesktop(pill) {
            Ok(on_current) if on_current.as_bool() => return,
            Ok(_) => {}
            Err(err) => {
                log_desktop_failure("check current desktop", &err);
                return;
            }
        }

        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return;
        }

        let desktop = match manager.GetWindowDesktopId(foreground) {
            Ok(desktop) => desktop,
            Err(err) => {
                log_desktop_failure("get foreground desktop", &err);
                return;
            }
        };

        if let Err(err) = manager.MoveWindowToDesktop(pill, &desktop) {
            log_desktop_failure("move window", &err);
        }
    }
}

fn log_desktop_failure(operation: &str, error: &Error) {
    if !DESKTOP_FAILURE_LOGGED.swap(true, Ordering::Relaxed) {
        eprintln!("virtual desktop {operation} failed: {error}");
    }
}

fn start_topmost_timer(hwnd: HWND) {
    let raw = hwnd.0 as isize;
    let spawned = thread::Builder::new()
        .name("topmost".to_string())
        .spawn(move || {
            let pill = HWND(raw as *mut c_void);
            let mut logged_failure = false;

            loop {
                thread::sleep(TOPMOST_INTERVAL);
                apply_styles_to_hwnd(pill);
                let result = unsafe {
                    SetWindowPos(
                        pill,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    )
                };
                if result.is_err() && !logged_failure {
                    eprintln!("failed to re-assert topmost; is the window gone?");
                    logged_failure = true;
                }
            }
        });

    if let Err(err) = spawned {
        eprintln!("failed to start topmost timer: {err}");
    }
}

fn restore_position(window: &WebviewWindow, hwnd: HWND) -> Result<(), String> {
    let settings = settings::load();

    let (x, y, monitor) = match settings.pos {
        Some(position) => {
            let probe = POINT {
                x: position.x + 100,
                y: position.y + 50,
            };
            match monitor_at(probe) {
                Some(monitor) => (position.x, position.y, monitor),
                None => default_position(hwnd)?,
            }
        }
        None => default_position(hwnd)?,
    };

    let (x, y) = clamp_to_monitor(x, y, &monitor);
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|err| err.to_string())
}

fn default_position(hwnd: HWND) -> Result<(i32, i32, MonitorWork), String> {
    let monitor =
        primary_monitor(hwnd).ok_or_else(|| "no primary monitor available".to_string())?;
    let margin = (DEFAULT_MARGIN * monitor.scale).round() as i32;
    let width = (WINDOW_WIDTH * monitor.scale).round() as i32;
    Ok((
        monitor.work.right - width - margin,
        monitor.work.top + margin,
        monitor,
    ))
}

fn clamp_to_monitor(x: i32, y: i32, monitor: &MonitorWork) -> (i32, i32) {
    let left_offset = (PILL_LEFT * monitor.scale).round() as i32;
    let top_offset = (PILL_TOP * monitor.scale).round() as i32;
    let pill_width = (PILL_MAX_WIDTH * monitor.scale).round() as i32;
    let pill_height = (PILL_HEIGHT * monitor.scale).round() as i32;

    let min_x = monitor.work.left - left_offset;
    let max_x = monitor.work.right - left_offset - pill_width;
    let min_y = monitor.work.top - top_offset;
    let max_y = monitor.work.bottom - top_offset - pill_height;

    let x = if max_x < min_x {
        min_x
    } else {
        x.clamp(min_x, max_x)
    };
    let y = if max_y < min_y {
        min_y
    } else {
        y.clamp(min_y, max_y)
    };

    (x, y)
}

fn monitor_at(point: POINT) -> Option<MonitorWork> {
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) };
    monitor_work(monitor)
}

fn primary_monitor(hwnd: HWND) -> Option<MonitorWork> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY) };
    monitor_work(monitor)
}

fn monitor_work(monitor: HMONITOR) -> Option<MonitorWork> {
    if monitor.is_invalid() {
        return None;
    }

    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };

    unsafe {
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }

        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        let scale = if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok()
        {
            dpi_x as f64 / 96.0
        } else {
            1.0
        };

        Some(MonitorWork {
            work: info.rcWork,
            scale,
        })
    }
}
