use std::time::Duration;

use serde::Deserialize;
use sky_core::SkyWeather;

const UA: &str = "SkyWallpaper/0.1 (https://github.com/sena-nana/SkyWallpaper)";
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, thiserror::Error)]
pub enum WeatherError {
    #[error("network: {0}")]
    Network(String),
    #[error("parse: {0}")]
    Parse(String),
    #[error("no results")]
    NoResults,
}

#[derive(Debug, Clone)]
pub struct GeoPlace {
    pub latitude: f64,
    pub longitude: f64,
    pub label: String,
    pub source: GeoSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoSource {
    Ip,
    Search,
    Manual,
}

#[derive(Deserialize)]
struct OpenMeteoCurrent {
    weather_code: Option<u8>,
    cloud_cover: Option<f32>,
    precipitation: Option<f32>,
    visibility: Option<f32>,
}

#[derive(Deserialize)]
struct OpenMeteoResponse {
    current: Option<OpenMeteoCurrent>,
}

#[derive(Deserialize)]
struct IpWho {
    success: Option<bool>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    city: Option<String>,
    country: Option<String>,
}

#[derive(Deserialize)]
struct GeoResult {
    name: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    country: Option<String>,
    admin1: Option<String>,
}

#[derive(Deserialize)]
struct GeoResponse {
    results: Option<Vec<GeoResult>>,
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(true)
        .build();
    ureq::Agent::new_with_config(config)
}

fn get_text(url: &str) -> Result<String, WeatherError> {
    let mut response = agent()
        .get(url)
        .header("User-Agent", UA)
        .call()
        .map_err(|err| WeatherError::Network(err.to_string()))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|err| WeatherError::Network(err.to_string()))
}

pub fn fetch_weather(latitude: f64, longitude: f64) -> Result<SkyWeather, WeatherError> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude:.4}&longitude={longitude:.4}&current=weather_code,cloud_cover,precipitation,visibility&timezone=auto"
    );
    let body = get_text(&url)?;
    let parsed: OpenMeteoResponse =
        serde_json::from_str(&body).map_err(|err| WeatherError::Parse(err.to_string()))?;
    let current = parsed.current.ok_or(WeatherError::NoResults)?;
    Ok(SkyWeather::from_wmo(
        current.weather_code.unwrap_or(0),
        current.cloud_cover.unwrap_or(0.0),
        current.precipitation.unwrap_or(0.0),
        current.visibility.unwrap_or(20_000.0),
    ))
}

pub fn lookup_ip() -> Result<GeoPlace, WeatherError> {
    let body = get_text("https://ipwho.is/")?;
    let parsed: IpWho =
        serde_json::from_str(&body).map_err(|err| WeatherError::Parse(err.to_string()))?;
    if parsed.success == Some(false) {
        return Err(WeatherError::NoResults);
    }
    let latitude = parsed.latitude.ok_or(WeatherError::NoResults)?;
    let longitude = parsed.longitude.ok_or(WeatherError::NoResults)?;
    let city = parsed.city.unwrap_or_default();
    let country = parsed.country.unwrap_or_default();
    let label = [city.as_str(), country.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(GeoPlace {
        latitude,
        longitude,
        label: if label.is_empty() {
            format!("{latitude:.2}, {longitude:.2}")
        } else {
            label
        },
        source: GeoSource::Ip,
    })
}

pub fn search_places(query: &str, language: &str) -> Result<Vec<GeoPlace>, WeatherError> {
    let q = urlencoding(query);
    let lang = if language.starts_with("zh") {
        "zh"
    } else {
        "en"
    };
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={q}&count=6&language={lang}&format=json"
    );
    let body = get_text(&url)?;
    let parsed: GeoResponse =
        serde_json::from_str(&body).map_err(|err| WeatherError::Parse(err.to_string()))?;
    let results = parsed.results.unwrap_or_default();
    if results.is_empty() {
        return Err(WeatherError::NoResults);
    }
    Ok(results
        .into_iter()
        .filter_map(|item| {
            let latitude = item.latitude?;
            let longitude = item.longitude?;
            let mut parts = Vec::new();
            if let Some(name) = item.name {
                if !name.is_empty() {
                    parts.push(name);
                }
            }
            if let Some(admin) = item.admin1 {
                if !admin.is_empty() && !parts.iter().any(|p| p == &admin) {
                    parts.push(admin);
                }
            }
            if let Some(country) = item.country {
                if !country.is_empty() {
                    parts.push(country);
                }
            }
            Some(GeoPlace {
                latitude,
                longitude,
                label: parts.join(", "),
                source: GeoSource::Search,
            })
        })
        .collect())
}

fn urlencoding(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(ch),
            ' ' => out.push_str("%20"),
            _ => {
                for byte in ch.encode_utf8(&mut [0; 4]).as_bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_meteo_sample() {
        let json = r#"{"current":{"weather_code":95,"cloud_cover":88,"precipitation":2.4,"visibility":4000}}"#;
        let parsed: OpenMeteoResponse = serde_json::from_str(json).unwrap();
        let c = parsed.current.unwrap();
        let w = SkyWeather::from_wmo(
            c.weather_code.unwrap(),
            c.cloud_cover.unwrap(),
            c.precipitation.unwrap(),
            c.visibility.unwrap(),
        );
        assert!(w.thunder);
        assert!(w.precip > 0.3);
        assert!(w.fog > 0.0);
    }

    #[test]
    fn parses_ipwho_sample() {
        let json = r#"{"success":true,"latitude":39.9,"longitude":116.4,"city":"Beijing","country":"China"}"#;
        let parsed: IpWho = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.city.as_deref(), Some("Beijing"));
    }
}
