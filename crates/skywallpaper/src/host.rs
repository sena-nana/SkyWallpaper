use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use sky_core::{SkyView, SkyWeather};
use wallpaper_host::{
    WallpaperEngine, foreground_is_fullscreen, on_battery, take_display_changed, target_fps,
    work_areas_occluded,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
};

use crate::config::LocationMode;
use crate::state::AppState;
use crate::tray::{self, Tray};

const HEALTH_INTERVAL: Duration = Duration::from_millis(500);
const WEATHER_INTERVAL: Duration = Duration::from_secs(20 * 60);
const PAUSED_SLEEP: Duration = Duration::from_millis(200);
const WAIT_SLICE: Duration = Duration::from_millis(16);
const ATTACH_RETRY_INTERVAL: Duration = Duration::from_secs(2);

pub fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let mut engine = loop {
        match WallpaperEngine::attach() {
            Ok(engine) => break engine,
            Err(err) => {
                state.set_status(format!("WorkerW: {err}; retrying"));
                eprintln!("wallpaper host failed: {err}; retrying in 2s");
                if pump_messages() || state.shutdown.load(Ordering::SeqCst) {
                    return Ok(());
                }
                std::thread::sleep(ATTACH_RETRY_INTERVAL);
            }
        }
    };

    let tray = tray::build(&state)?;
    spawn_weather_worker(state.clone());

    let mut last_frame = Instant::now();
    let mut last_weather = Instant::now();
    let mut last_health = Instant::now();
    let mut rt = Runtime::new(&state, &mut engine);

    loop {
        if pump_messages() {
            break;
        }
        handle_tray(&state, &tray);

        if state.shutdown.load(Ordering::SeqCst) {
            break;
        }

        if last_health.elapsed() >= HEALTH_INTERVAL {
            last_health = Instant::now();
            rt.refresh(&state, &mut engine);
        }

        if last_weather.elapsed() >= WEATHER_INTERVAL {
            state.refresh_weather.store(true, Ordering::Relaxed);
            last_weather = Instant::now();
        }

        let fps = target_fps(state.is_paused(), rt.battery, rt.fullscreen, rt.occluded);
        if fps == 0 {
            std::thread::sleep(PAUSED_SLEEP);
            continue;
        }
        let interval = Duration::from_secs_f64(1.0 / f64::from(fps));
        let elapsed = last_frame.elapsed();
        if elapsed < interval {
            std::thread::sleep((interval - elapsed).min(WAIT_SLICE));
            continue;
        }
        last_frame = Instant::now();

        let weather = state.effective_weather();
        if weather.thunder && engine.thunder_flash < 0.05 && hash_time(engine.elapsed()) < 0.01 {
            engine.thunder_flash = 1.0;
            engine.thunder_seed = engine.elapsed();
        }
        engine.thunder_flash *= 0.84;

        let view = SkyView::now(rt.latitude, rt.longitude, weather);
        if let Err(err) = engine.render(&view, &rt.skip) {
            state.set_status(err.to_string());
        }
    }

    engine.teardown();
    Ok(())
}

struct Runtime {
    latitude: f64,
    longitude: f64,
    battery: bool,
    fullscreen: bool,
    skip: Vec<bool>,
    occluded: bool,
    layout: Vec<(i32, i32, u32, u32)>,
}

impl Runtime {
    fn new(state: &AppState, engine: &mut WallpaperEngine) -> Self {
        let mut rt = Self {
            latitude: 0.0,
            longitude: 0.0,
            battery: false,
            fullscreen: false,
            skip: Vec::new(),
            occluded: false,
            layout: engine.monitor_layout(),
        };
        rt.refresh(state, engine);
        rt
    }

    fn refresh(&mut self, state: &AppState, engine: &mut WallpaperEngine) {
        let pause_on_fullscreen = {
            let cfg = state.config.lock().unwrap();
            self.latitude = cfg.latitude;
            self.longitude = cfg.longitude;
            cfg.pause_on_fullscreen
        };
        recover_host(state, engine, &mut self.layout);
        self.battery = on_battery();
        self.fullscreen = pause_on_fullscreen && foreground_is_fullscreen();
        self.skip = work_areas_occluded(&engine.slot_work_areas());
        self.occluded = !self.skip.is_empty() && self.skip.iter().all(|hidden| *hidden);
    }
}

fn recover_host(
    state: &AppState,
    engine: &mut WallpaperEngine,
    layout: &mut Vec<(i32, i32, u32, u32)>,
) {
    let layout_now = engine.monitor_layout();
    let dead = !engine.parent_alive();
    let wrong = engine.slot_count() != layout_now.len();
    if !dead && !wrong && !take_display_changed() && layout_now == *layout {
        return;
    }
    match engine.recover() {
        Ok(()) => {
            *layout = engine.monitor_layout();
            if dead || wrong {
                state.set_status("WorkerW reattached");
            }
        }
        Err(err) => state.set_status(format!("WorkerW: {err}")),
    }
}

fn spawn_weather_worker(state: Arc<AppState>) {
    let worker = state.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("skywallpaper-weather".into())
        .spawn(move || weather_loop(worker))
    {
        eprintln!("weather worker: {err}");
        bootstrap_location(&state);
        refresh_weather(&state);
    }
}

fn weather_loop(state: Arc<AppState>) {
    bootstrap_location(&state);
    loop {
        if state.shutdown.load(Ordering::SeqCst) {
            break;
        }
        if state.refresh_weather.swap(false, Ordering::Relaxed) {
            refresh_weather(&state);
            continue;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

fn bootstrap_location(state: &AppState) {
    let mode = state.config.lock().unwrap().location_mode;
    if mode != LocationMode::Ip {
        return;
    }
    match weather::lookup_ip() {
        Ok(place) => {
            state.update_location(
                place.latitude,
                place.longitude,
                place.label,
                LocationMode::Ip,
            );
        }
        Err(err) => state.set_status(format!("IP: {err}")),
    }
}

fn refresh_weather(state: &AppState) {
    let (lat, lon) = {
        let cfg = state.config.lock().unwrap();
        (cfg.latitude, cfg.longitude)
    };
    match weather::fetch_weather(lat, lon) {
        Ok(weather) => {
            *state.weather.lock().unwrap() = weather;
            state.set_status(format!("WMO {}", weather.code.0));
        }
        Err(err) => {
            *state.weather.lock().unwrap() = SkyWeather::clear_fallback();
            state.set_status(format!("weather fallback: {err}"));
        }
    }
}

fn handle_tray(state: &Arc<AppState>, tray: &Tray) {
    if tray::poll_tray_click() {
        tray::open_settings(state.clone());
    }
    if let Some(id) = tray::poll_menu() {
        if id == tray.ids.settings {
            tray::open_settings(state.clone());
        } else if id == tray.ids.pause {
            state.toggle_pause();
            tray.sync_pause(state);
        } else if id == tray.ids.quit {
            state.request_shutdown();
        }
    }
}

fn pump_messages() -> bool {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return true;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    false
}

fn hash_time(t: f32) -> f32 {
    let n = (t * 19.19).sin() * 43758.5453;
    n.fract().abs()
}
