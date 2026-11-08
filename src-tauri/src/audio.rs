use std::io::Cursor;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use rodio::{Decoder, DeviceSinkBuilder, Player};

use crate::engine::Phase;
use crate::settings::Settings;

const FOCUS_END: &[u8] = include_bytes!("../sounds/focus_end.wav");
const BREAK_END: &[u8] = include_bytes!("../sounds/break_end.wav");

pub fn spawn(settings: Arc<Mutex<Settings>>) -> Sender<Phase> {
    let (tx, rx) = mpsc::channel();

    let spawned = thread::Builder::new()
        .name("audio".to_string())
        .spawn(move || {
            let device = match DeviceSinkBuilder::open_default_sink() {
                Ok(device) => device,
                Err(err) => {
                    eprintln!("audio device unavailable; sounds disabled: {err}");
                    return;
                }
            };

            for phase in rx {
                let (enabled, volume) = match settings.lock() {
                    Ok(settings) => (settings.sound, settings.volume),
                    Err(_) => (true, 0.6),
                };
                if !enabled {
                    continue;
                }

                let bytes = match phase {
                    Phase::Focus => FOCUS_END,
                    Phase::Break => BREAK_END,
                };

                match Decoder::new(Cursor::new(bytes)) {
                    Ok(source) => {
                        let player = Player::connect_new(device.mixer());
                        player.set_volume(volume.clamp(0.0, 1.0));
                        player.append(source);
                        player.detach();
                    }
                    Err(err) => eprintln!("failed to decode {:?} end sound: {err}", phase),
                }
            }
        });

    if let Err(err) = spawned {
        eprintln!("failed to start audio thread: {err}");
    }

    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_sounds_decode() {
        for (name, bytes) in [("focus", FOCUS_END), ("break", BREAK_END)] {
            let decoder = Decoder::new(Cursor::new(bytes)).expect("decode embedded wav");
            assert!(decoder.count() > 0, "{name} sound has no samples");
        }
    }
}
