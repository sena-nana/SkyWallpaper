mod autostart;
mod config;
mod debug;
mod host;
mod i18n;
mod preview;
mod settings;
mod shell;
mod state;
mod tray;

use std::sync::{Arc, Mutex, OnceLock};

use sky_core::SkyWeather;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

static INSTANCE_MUTEX: OnceLock<isize> = OnceLock::new();

use crate::config::Config;
use crate::preview::PreviewOpts;
use crate::state::AppState;

fn main() {
    if let Err(err) = real_main() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn real_main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let preview = args.iter().any(|a| a == "--preview");
    let debug_mode = args.iter().any(|a| a == "--debug");
    let debug_panel = args.iter().any(|a| a == "--debug-panel");
    let weather = parse_weather(&args);
    let (lat, lon) = parse_lat_lon(&args);
    let debug_params_path = parse_path_flag(&args, "--debug-params");

    let mut config = Config::load();
    if let Some(lat) = lat {
        config.latitude = lat;
    }
    if let Some(lon) = lon {
        config.longitude = lon;
    }

    if debug_panel {
        let weather = weather.unwrap_or_else(SkyWeather::clear_fallback);
        let params = debug_params_path
            .as_ref()
            .and_then(|path| debug::DebugParams::load(path))
            .unwrap_or_else(|| {
                debug::DebugParams::from_live(config.latitude, config.longitude, weather)
            });
        return debug::run(Arc::new(Mutex::new(params)), debug_params_path);
    }

    if preview || debug_mode {
        let weather = weather.unwrap_or_else(SkyWeather::clear_fallback);
        let (debug_path, debug_panel) = if debug_mode {
            spawn_debug_panel(config.latitude, config.longitude, weather)?
        } else {
            (None, None)
        };
        return preview::run(PreviewOpts {
            latitude: config.latitude,
            longitude: config.longitude,
            weather,
            debug_path,
            debug_panel,
        });
    }

    acquire_single_instance()?;
    let state = AppState::new(config);
    if let Some(weather) = weather {
        *state.weather_override.lock().unwrap() = Some(weather);
    }
    if state.config.lock().unwrap().autostart {
        let _ = autostart::set_enabled(true);
    }
    host::run(state)
}

fn parse_weather(args: &[String]) -> Option<SkyWeather> {
    args.windows(2).find_map(|pair| {
        if pair[0] == "--weather" {
            Some(debug::weather_preset(&pair[1]))
        } else {
            None
        }
    })
}

fn parse_path_flag(args: &[String], flag: &str) -> Option<std::path::PathBuf> {
    args.windows(2).find_map(|pair| {
        if pair[0] == flag {
            Some(std::path::PathBuf::from(&pair[1]))
        } else {
            None
        }
    })
}

fn spawn_debug_panel(
    latitude: f64,
    longitude: f64,
    weather: SkyWeather,
) -> anyhow::Result<(Option<std::path::PathBuf>, Option<std::process::Child>)> {
    let path = std::env::temp_dir().join(format!("skywallpaper-debug-{}.toml", std::process::id()));
    let params = debug::DebugParams::from_live(latitude, longitude, weather);
    params.save(&path)?;
    let exe = std::env::current_exe()?;
    let child = std::process::Command::new(exe)
        .arg("--debug-panel")
        .arg("--debug-params")
        .arg(&path)
        .spawn()?;
    Ok((Some(path), Some(child)))
}

fn parse_lat_lon(args: &[String]) -> (Option<f64>, Option<f64>) {
    let mut lat = None;
    let mut lon = None;
    let mut i = 0;
    while i + 1 < args.len() {
        match args[i].as_str() {
            "--lat" => lat = args[i + 1].parse().ok(),
            "--lon" => lon = args[i + 1].parse().ok(),
            _ => {
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    (lat, lon)
}

fn acquire_single_instance() -> anyhow::Result<()> {
    unsafe {
        let mutex = CreateMutexW(None, true, w!("Local\\SkyWallpaper.SingleInstance"))?;
        if GetLastError().0 == 183 {
            anyhow::bail!("SkyWallpaper is already running");
        }
        let _ = INSTANCE_MUTEX.set(mutex.0 as isize);
    }
    Ok(())
}
