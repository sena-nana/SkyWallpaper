use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use sky_core::SkyWeather;

use crate::config::{Config, LanguagePref, LocationMode};
use crate::i18n::{Lang, Text, t};

pub struct AppState {
    pub config: Mutex<Config>,
    pub weather: Mutex<SkyWeather>,
    pub weather_override: Mutex<Option<SkyWeather>>,
    pub paused: AtomicBool,
    pub shutdown: AtomicBool,
    pub settings_open: AtomicBool,
    pub refresh_weather: AtomicBool,
    pub status: Mutex<String>,
}

impl AppState {
    pub fn new(config: Config) -> Arc<Self> {
        Arc::new(Self {
            config: Mutex::new(config),
            weather: Mutex::new(SkyWeather::clear_fallback()),
            weather_override: Mutex::new(None),
            paused: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            settings_open: AtomicBool::new(false),
            refresh_weather: AtomicBool::new(true),
            status: Mutex::new(String::new()),
        })
    }

    pub fn lang(&self) -> Lang {
        Lang::resolve(self.config.lock().unwrap().language)
    }

    pub fn text(&self) -> Text {
        t(self.lang())
    }

    pub fn effective_weather(&self) -> SkyWeather {
        if let Some(over) = *self.weather_override.lock().unwrap() {
            return over;
        }
        *self.weather.lock().unwrap()
    }

    pub fn set_status(&self, value: impl Into<String>) {
        *self.status.lock().unwrap() = value.into();
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn toggle_pause(&self) {
        let next = !self.paused.load(Ordering::Relaxed);
        self.paused.store(next, Ordering::Relaxed);
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn update_location(&self, lat: f64, lon: f64, label: String, mode: LocationMode) {
        let mut cfg = self.config.lock().unwrap();
        cfg.latitude = lat;
        cfg.longitude = lon;
        cfg.label = label;
        cfg.location_mode = mode;
        cfg.save();
        self.refresh_weather.store(true, Ordering::Relaxed);
    }

    pub fn set_language(&self, pref: LanguagePref) {
        let mut cfg = self.config.lock().unwrap();
        cfg.language = pref;
        cfg.save();
    }
}
