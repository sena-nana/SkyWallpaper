/// WMO weather interpretation code (Open-Meteo `weather_code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherCode(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecipKind {
    Rain,
    Snow,
}

/// Shader-facing weather, all fields 0..=1 unless noted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyWeather {
    pub code: WeatherCode,
    pub cloud_cover: f32,
    pub precip: f32,
    pub precip_kind: PrecipKind,
    pub fog: f32,
    pub thunder: bool,
}

impl SkyWeather {
    pub fn clear_fallback() -> Self {
        Self {
            code: WeatherCode(0),
            cloud_cover: 0.12,
            precip: 0.0,
            precip_kind: PrecipKind::Rain,
            fog: 0.0,
            thunder: false,
        }
    }

    /// `cloud_cover` 0..=100, `precipitation` mm, `visibility` metres (Open-Meteo).
    pub fn from_wmo(
        code: u8,
        cloud_cover_pct: f32,
        precipitation_mm: f32,
        visibility_m: f32,
    ) -> Self {
        let thunder = matches!(code, 95 | 96 | 99);
        let snow = matches!(code, 71..=77 | 85 | 86);
        let foggy = matches!(code, 45 | 48);
        let overcast = matches!(code, 3 | 51..=99);
        let mut cloud = (cloud_cover_pct / 100.0).clamp(0.0, 1.0);
        if overcast {
            cloud = cloud.max(0.55);
        }
        if matches!(code, 0) {
            cloud = cloud.min(0.2);
        }
        let mut precip = (precipitation_mm / 6.0).clamp(0.0, 1.0);
        precip = match code {
            51 | 56 | 80 => precip.max(0.18),
            53 | 61 | 81 => precip.max(0.35),
            55 | 63 | 65 | 82 => precip.max(0.65),
            66 | 67 => precip.max(0.4),
            71 | 85 => precip.max(0.25),
            73 | 75 | 77 | 86 => precip.max(0.55),
            95 => precip.max(0.4),
            96 | 99 => precip.max(0.7),
            _ => precip,
        };
        if matches!(code, 0 | 1 | 2 | 3) {
            precip = 0.0;
        }
        let mut fog = if foggy {
            0.75
        } else {
            (1.0 - (visibility_m / 12_000.0)).clamp(0.0, 0.85)
        };
        if visibility_m <= 0.0 {
            fog = if foggy { 0.75 } else { 0.0 };
        }
        Self {
            code: WeatherCode(code),
            cloud_cover: cloud,
            precip,
            precip_kind: if snow {
                PrecipKind::Snow
            } else {
                PrecipKind::Rain
            },
            fog,
            thunder,
        }
    }
}
