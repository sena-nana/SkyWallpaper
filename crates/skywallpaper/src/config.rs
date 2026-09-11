use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LocationMode {
    #[default]
    Ip,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LanguagePref {
    #[default]
    System,
    Zh,
    En,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub latitude: f64,
    pub longitude: f64,
    pub label: String,
    pub location_mode: LocationMode,
    pub language: LanguagePref,
    pub autostart: bool,
    pub pause_on_fullscreen: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            latitude: 39.9042,
            longitude: 116.4074,
            label: String::from("Beijing"),
            location_mode: LocationMode::Ip,
            language: LanguagePref::System,
            autostart: false,
            pause_on_fullscreen: true,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs_config().join("SkyWallpaper").join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = toml::to_string_pretty(self) {
            let _ = fs::write(path, text);
        }
    }
}

fn dirs_config() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}


