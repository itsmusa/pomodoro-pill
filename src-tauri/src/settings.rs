use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub focus_min: u32,
    pub break_min: u32,
    pub idle_reset_min: u32,
    pub sound: bool,
    pub volume: f32,
    pub autostart: bool,
    pub hide_in_fullscreen: bool,
    pub pos: Option<Position>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            focus_min: 25,
            break_min: 5,
            idle_reset_min: 15,
            sound: true,
            volume: 0.6,
            autostart: false,
            hide_in_fullscreen: false,
            pos: None,
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        self.focus_min = self.focus_min.max(1);
        self.break_min = self.break_min.max(1);
        self.idle_reset_min = self.idle_reset_min.max(1);
        self.volume = self.volume.clamp(0.0, 1.0);
    }
}

pub fn path() -> Result<PathBuf, String> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| "APPDATA is not set".to_string())?;
    Ok(PathBuf::from(appdata)
        .join("PomodoroPill")
        .join("settings.json"))
}

pub fn load() -> Settings {
    let path = match path() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("cannot locate settings file: {err}");
            return Settings::default();
        }
    };

    match fs::read_to_string(&path) {
        Ok(text) => {
            let text = text.trim_start_matches('\u{feff}');
            match serde_json::from_str::<Settings>(text) {
                Ok(mut settings) => {
                    settings.sanitize();
                    settings
                }
                Err(err) => {
                    eprintln!("settings file is corrupt ({}): {err}", path.display());
                    Settings::default()
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(err) => {
            eprintln!("failed to read settings file ({}): {err}", path.display());
            Settings::default()
        }
    }
}

pub fn save(settings: &Settings) -> Result<(), String> {
    write(settings)
}

pub fn save_position(position: Position) -> Result<(), String> {
    let mut settings = load();
    settings.pos = Some(position);
    write(&settings)
}

pub fn clear_position() -> Result<(), String> {
    let mut settings = load();
    settings.pos = None;
    write(&settings)
}

pub fn ensure_file() -> Result<PathBuf, String> {
    let path = path()?;
    if !path.exists() {
        write(&Settings::default())?;
    }
    Ok(path)
}

fn write(settings: &Settings) -> Result<(), String> {
    let path = path()?;
    let parent = path
        .parent()
        .ok_or_else(|| "settings path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|err| format!("create settings directory: {err}"))?;

    let mut sanitized = settings.clone();
    sanitized.sanitize();

    let temp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&sanitized)
        .map_err(|err| format!("serialize settings: {err}"))?;
    fs::write(&temp, text).map_err(|err| format!("write settings temporary file: {err}"))?;
    fs::rename(&temp, &path).map_err(|err| format!("replace settings file: {err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_enforces_minimums_and_volume_range() {
        let mut settings = Settings {
            focus_min: 0,
            break_min: 0,
            idle_reset_min: 0,
            volume: 2.0,
            ..Settings::default()
        };
        settings.sanitize();
        assert_eq!(settings.focus_min, 1);
        assert_eq!(settings.break_min, 1);
        assert_eq!(settings.idle_reset_min, 1);
        assert!((settings.volume - 1.0).abs() < f32::EPSILON);

        settings.volume = -1.0;
        settings.sanitize();
        assert!(settings.volume.abs() < f32::EPSILON);
    }
}
