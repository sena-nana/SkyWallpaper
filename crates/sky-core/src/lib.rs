//! Sky state from observer location and UTC time.

mod moon;
mod sun;
mod weather_map;

pub use moon::MoonState;
pub use sun::SunState;
pub use weather_map::{PrecipKind, SkyWeather, WeatherCode};

use chrono::{DateTime, Utc};

const DEG: f64 = std::f64::consts::PI / 180.0;
const RAD: f64 = 180.0 / std::f64::consts::PI;

/// Julian Date from a UTC instant (UTC, not TT; enough for sky wallpaper).
pub fn julian_date(utc: DateTime<Utc>) -> f64 {
    let seconds = utc.timestamp_millis() as f64 / 1000.0;
    seconds / 86_400.0 + 2_440_587.5
}

pub fn julian_centuries(jd: f64) -> f64 {
    (jd - 2_451_545.0) / 36_525.0
}

fn wrap_deg(mut deg: f64) -> f64 {
    deg %= 360.0;
    if deg < 0.0 {
        deg += 360.0;
    }
    deg
}

fn sind(deg: f64) -> f64 {
    (deg * DEG).sin()
}
fn cosd(deg: f64) -> f64 {
    (deg * DEG).cos()
}

/// Local mean sidereal time in degrees.
pub fn local_sidereal_deg(jd: f64, longitude_deg: f64) -> f64 {
    let d = jd - 2_451_545.0;
    let gmst = wrap_deg(280.46061837 + 360.98564736629 * d);
    wrap_deg(gmst + longitude_deg)
}

/// East, Up, North unit vector from altitude (deg) and azimuth (deg from north, clockwise).
pub fn enu_from_alt_az(altitude_deg: f64, azimuth_deg: f64) -> [f32; 3] {
    let alt = altitude_deg * DEG;
    let az = azimuth_deg * DEG;
    let east = az.sin() * alt.cos();
    let up = alt.sin();
    let north = az.cos() * alt.cos();
    [east as f32, up as f32, north as f32]
}

fn equatorial_to_altaz(
    ra_deg: f64,
    dec_deg: f64,
    lst_deg: f64,
    latitude_deg: f64,
) -> (f64, f64) {
    let ha = wrap_deg(lst_deg - ra_deg);
    let sin_alt = sind(latitude_deg) * sind(dec_deg) + cosd(latitude_deg) * cosd(dec_deg) * cosd(ha);
    let altitude = sin_alt.clamp(-1.0, 1.0).asin() * RAD;
    let y = sind(ha);
    let x = cosd(ha) * sind(latitude_deg) - tand(dec_deg) * cosd(latitude_deg);
    let azimuth = wrap_deg(y.atan2(x) * RAD + 180.0);
    (altitude, azimuth)
}

fn tand(deg: f64) -> f64 {
    (deg * DEG).tan()
}

/// Combined observer sky for one frame.
#[derive(Debug, Clone)]
pub struct SkyView {
    pub sun: SunState,
    pub moon: MoonState,
    pub sidereal_deg: f64,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub weather: SkyWeather,
    pub cam_pitch_deg: f32,
    pub cam_yaw_deg: f32,
    pub exposure: f32,
}

impl SkyView {
    pub fn at(
        latitude_deg: f64,
        longitude_deg: f64,
        utc: DateTime<Utc>,
        weather: SkyWeather,
    ) -> Self {
        let jd = julian_date(utc);
        let lst = local_sidereal_deg(jd, longitude_deg);
        let sun = SunState::at(latitude_deg, longitude_deg, utc);
        let moon = MoonState::at(latitude_deg, longitude_deg, utc);
        let night = ((-sun.altitude_deg - 2.0) / 10.0).clamp(0.0, 1.0) as f32;
        let yaw = if night > 0.55 {
            moon.azimuth_deg as f32
        } else {
            sun.azimuth_deg as f32
        };
        let pitch = 18.0;
        let mut exposure = 1.0;
        if sun.altitude_deg < -6.0 {
            exposure = 1.35;
        } else if sun.altitude_deg < 0.0 {
            exposure = 1.15;
        }
        if weather.fog > 0.4 {
            exposure *= 0.92;
        }
        Self {
            sun,
            moon,
            sidereal_deg: lst,
            latitude_deg,
            longitude_deg,
            weather,
            cam_pitch_deg: pitch,
            cam_yaw_deg: yaw,
            exposure,
        }
    }

    pub fn now(latitude_deg: f64, longitude_deg: f64, weather: SkyWeather) -> Self {
        Self::at(latitude_deg, longitude_deg, Utc::now(), weather)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const BEIJING: (f64, f64) = (39.9042, 116.4074);

    #[test]
    fn beijing_summer_solstice_noon_is_high() {
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 4, 15, 0).unwrap();
        let sun = SunState::at(BEIJING.0, BEIJING.1, t);
        assert!(
            sun.altitude_deg > 70.0 && sun.altitude_deg < 76.0,
            "alt {}",
            sun.altitude_deg
        );
    }

    #[test]
    fn beijing_winter_solstice_noon_is_low() {
        let t = Utc.with_ymd_and_hms(2024, 12, 21, 4, 20, 0).unwrap();
        let sun = SunState::at(BEIJING.0, BEIJING.1, t);
        assert!(
            sun.altitude_deg > 24.0 && sun.altitude_deg < 31.0,
            "alt {}",
            sun.altitude_deg
        );
    }

    #[test]
    fn equator_equinox_noon_near_zenith() {
        let t = Utc.with_ymd_and_hms(2024, 3, 20, 12, 8, 0).unwrap();
        let sun = SunState::at(0.0, 0.0, t);
        assert!(sun.altitude_deg > 85.0, "alt {}", sun.altitude_deg);
    }

    #[test]
    fn beijing_midnight_is_night() {
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 16, 0, 0).unwrap();
        let sun = SunState::at(BEIJING.0, BEIJING.1, t);
        assert!(sun.altitude_deg < -10.0, "alt {}", sun.altitude_deg);
    }

    #[test]
    fn full_moon_near_june_22_2024() {
        let t = Utc.with_ymd_and_hms(2024, 6, 22, 1, 8, 0).unwrap();
        let moon = MoonState::at(0.0, 0.0, t);
        assert!(moon.illumination > 0.95, "illum {}", moon.illumination);
    }

    #[test]
    fn new_moon_near_june_6_2024() {
        let t = Utc.with_ymd_and_hms(2024, 6, 6, 12, 38, 0).unwrap();
        let moon = MoonState::at(0.0, 0.0, t);
        assert!(moon.illumination < 0.08, "illum {}", moon.illumination);
    }

    #[test]
    fn thunder_maps_from_wmo_95() {
        let w = SkyWeather::from_wmo(95, 80.0, 2.0, 8_000.0);
        assert!(w.thunder);
        assert!(w.precip > 0.2);
        assert_eq!(w.precip_kind, PrecipKind::Rain);
    }

    #[test]
    fn snow_maps_from_wmo_73() {
        let w = SkyWeather::from_wmo(73, 90.0, 1.5, 4_000.0);
        assert_eq!(w.precip_kind, PrecipKind::Snow);
        assert!(w.cloud_cover > 0.7);
    }

    #[test]
    fn clear_fallback_is_fair() {
        let w = SkyWeather::clear_fallback();
        assert!(!w.thunder);
        assert_eq!(w.precip, 0.0);
        assert!(w.cloud_cover < 0.25);
    }
}
