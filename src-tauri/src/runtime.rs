use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::engine::{Engine, Event, Phase, Snapshot};

const TICK_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug)]
pub enum RuntimeSignal {
    Suspend,
    Toggle,
}

/// Starts the 250 ms tick loop. The loop owns no Tauri state during the lock;
/// it clones the snapshot, drops the guard, then emits.
pub fn spawn_tick_loop(
    app: AppHandle,
    engine: Arc<Mutex<Engine>>,
    signal_rx: Receiver<RuntimeSignal>,
    audio_tx: Sender<Phase>,
) {
    thread::spawn(move || {
        let mut last_snapshot: Option<Snapshot> = None;

        loop {
            let now = Instant::now();
            let (snapshot, events) = {
                let mut guard = match engine.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        eprintln!("engine lock poisoned; recovering");
                        poisoned.into_inner()
                    }
                };
                while let Ok(signal) = signal_rx.try_recv() {
                    match signal {
                        RuntimeSignal::Suspend => guard.suspend(now),
                        RuntimeSignal::Toggle => guard.toggle(now),
                    }
                }
                let events = guard.tick(now);
                (guard.snapshot(now), events)
            };

            if last_snapshot.as_ref() != Some(&snapshot) {
                if let Err(err) = app.emit("engine://snapshot", &snapshot) {
                    eprintln!("failed to emit snapshot: {err}");
                }
                last_snapshot = Some(snapshot);
            }

            for event in events {
                if let Event::PhaseEnded(phase) = event {
                    if let Err(err) = app.emit("engine://phase_ended", phase) {
                        eprintln!("failed to emit phase end: {err}");
                    }
                    if let Err(err) = audio_tx.send(phase) {
                        eprintln!("audio channel is closed: {err}");
                    }
                }
            }

            thread::sleep(TICK_INTERVAL);
        }
    });
}
