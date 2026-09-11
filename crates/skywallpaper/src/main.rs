mod autostart;
mod config;
mod host;
mod i18n;
mod preview;
mod settings;
mod state;
mod tray;

use std::sync::OnceLock;

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
    let weather = parse_weather(&args);
    let (lat, lon) = parse_lat_lon(&args);

    let mut config = Config::load();
    if let Some(lat) = lat {
        config.latitude = lat;
    }
    if let Some(lon) = lon {
        config.longitude = lon;
    }

    if preview {
        let weather = weather.unwrap_or_else(SkyWeather::clear_fallback);
        return preview::run(PreviewOpts {
            latitude: config.latitude,
            longitude: config.longitude,
            weather,
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
    let raw = args.windows(2).find_map(|pair| {
        if pair[0] == "--weather" {
            Some(pair[1].as_str())
        } else {
            None
        }
    })?;
    Some(match raw {
        "clear" => SkyWeather::clear_fallback(),
        "cloud" | "cloudy" => SkyWeather::from_wmo(3, 85.0, 0.0, 16_000.0),
        "rain" => SkyWeather::from_wmo(63, 95.0, 4.0, 8_000.0),
        "snow" => SkyWeather::from_wmo(73, 95.0, 2.0, 6_000.0),
        "fog" => SkyWeather::from_wmo(45, 80.0, 0.0, 400.0),
        "thunder" | "storm" => SkyWeather::from_wmo(95, 100.0, 5.0, 4_000.0),
        _ => SkyWeather::clear_fallback(),
    })
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
