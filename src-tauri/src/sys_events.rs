#![cfg(windows)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use std::thread;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    PostQuitMessage, RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, PBT_APMSUSPEND,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW,
    WTS_SESSION_LOCK, WTS_SESSION_LOGOFF,
};

use crate::runtime::RuntimeSignal;

static SUSPEND_TX: OnceLock<Sender<RuntimeSignal>> = OnceLock::new();
static SESSION_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn spawn(suspend_tx: Sender<RuntimeSignal>) {
    if SUSPEND_TX.set(suspend_tx).is_err() {
        eprintln!("session events already started");
        return;
    }

    let spawned = thread::Builder::new()
        .name("session-events".to_string())
        .spawn(run_message_loop);

    if let Err(err) = spawned {
        eprintln!("failed to start session event thread: {err}");
    }
}

pub fn shutdown() {
    let raw = SESSION_HWND.load(Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    let hwnd = HWND(raw as *mut c_void);
    if let Err(err) = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
        eprintln!("failed to close session event window: {err}");
    }
}

fn run_message_loop() {
    unsafe {
        let instance = match GetModuleHandleW(None) {
            Ok(module) => HINSTANCE(module.0),
            Err(err) => {
                eprintln!("failed to get module handle for session events: {err}");
                return;
            }
        };

        let class_name = w!("PomodoroPillSession");
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };

        if RegisterClassW(&class) == 0 {
            eprintln!("failed to register session event window class");
            return;
        }

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(err) => {
                eprintln!("failed to create session event window: {err}");
                return;
            }
        };

        SESSION_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

        if let Err(err) = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) {
            eprintln!("failed to register session notifications: {err}");
        }

        let mut msg = MSG::default();
        loop {
            let result = GetMessageW(&mut msg, None, 0, 0);
            if result.0 == 0 || result.0 == -1 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        SESSION_HWND.store(0, Ordering::Relaxed);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_WTSSESSION_CHANGE => {
            let reason = wparam.0 as u32;
            if reason == WTS_SESSION_LOCK || reason == WTS_SESSION_LOGOFF {
                notify_suspend();
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wparam.0 as u32 == PBT_APMSUSPEND {
                notify_suspend();
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = WTSUnRegisterSessionNotification(hwnd);
            let _ = DestroyWindow(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn notify_suspend() {
    let Some(tx) = SUSPEND_TX.get() else {
        return;
    };
    if tx.send(RuntimeSignal::Suspend).is_err() {
        eprintln!("session suspend channel is closed");
    }
}
