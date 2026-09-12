use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use sky_core::{SkyView, SkyWeather};
use wallpaper_host::{
    WallpaperEngine, foreground_is_fullscreen, on_battery, take_display_changed, target_fps,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
};

use crate::config::LocationMode;
use crate::state::AppState;
use crate::tray::{self, Tray};

pub fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let mut engine = match WallpaperEngine::attach() {
        Ok(engine) => engine,
        Err(err) => {
            state.set_status(format!("WorkerW: {err}"));
            eprintln!("wallpaper host failed: {err}");
            eprintln!("falling back to --preview window");
            let cfg = state.config.lock().unwrap().clone();
            return crate::preview::run(crate::preview::PreviewOpts {
                latitude: cfg.latitude,
                longitude: cfg.longitude,
                weather: state.effective_weather(),
                debug_path: None,
                debug_panel: None,
            });
        }
    };

    bootstrap_location(&state);
    refresh_weather(&state);

    let tray = tray::build(&state)?;
    let mut last_frame = Instant::now();
    let mut last_weather = Instant::now();

    loop {
        if pump_messages() {
            break;
        }
        handle_tray(&state, &tray);

        if state.shutdown.load(Ordering::SeqCst) {
            break;
        }

        if take_display_changed() {
            let _ = engine.rebuild();
        }

        if state.refresh_weather.swap(false, Ordering::Relaxed)
            || last_weather.elapsed() > Duration::from_secs(20 * 60)
        {
            refresh_weather(&state);
            last_weather = Instant::now();
        }

        let cfg = state.config.lock().unwrap().clone();
        let fullscreen = cfg.pause_on_fullscreen && foreground_is_fullscreen();
        let fps = target_fps(state.is_paused(), on_battery(), fullscreen);
        if fps == 0 {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        let interval = Duration::from_secs_f64(1.0 / f64::from(fps));
        if last_frame.elapsed() < interval {
            std::thread::sleep(Duration::from_millis(4));
            continue;
        }
        last_frame = Instant::now();

        let weather = state.effective_weather();
        if weather.thunder && engine.thunder_flash < 0.05 && hash_time(engine.elapsed()) < 0.01 {
            engine.thunder_flash = 1.0;
            engine.thunder_seed = engine.elapsed();
        }
        engine.thunder_flash *= 0.84;

        let view = SkyView::now(cfg.latitude, cfg.longitude, weather);
        if let Err(err) = engine.render(&view) {
            state.set_status(err.to_string());
        }
    }

    engine.teardown();
    Ok(())
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
