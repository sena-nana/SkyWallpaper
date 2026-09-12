//! Sky state from observer location and UTC time.

mod sun;
mod weather_map;

pub use sun::SunState;
pub use weather_map::{PrecipKind, SkyWeather, WeatherCode};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};

const DEG: f64 = std::f64::consts::PI / 180.0;
const RAD: f64 = 180.0 / std::f64::consts::PI;

fn julian_date(utc: DateTime<Utc>) -> f64 {
    let seconds = utc.timestamp_millis() as f64 / 1000.0;
    seconds / 86_400.0 + 2_440_587.5
}

fn julian_centuries(jd: f64) -> f64 {
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

fn local_sidereal_deg(jd: f64, longitude_deg: f64) -> f64 {
    let d = jd - 2_451_545.0;
    let gmst = wrap_deg(280.46061837 + 360.98564736629 * d);
    wrap_deg(gmst + longitude_deg)
}

/// Horizon sun in the 2D sky plane: `[cos(alt), sin(alt), 0]`.
pub fn sun_dir_2d(altitude_deg: f64) -> [f32; 3] {
    let alt = altitude_deg * DEG;
    [alt.cos() as f32, alt.sin() as f32, 0.0]
}

fn solar_altitude_deg(ra_deg: f64, dec_deg: f64, lst_deg: f64, latitude_deg: f64) -> f64 {
    let ha = wrap_deg(lst_deg - ra_deg);
    let sin_alt = sind(latitude_deg) * sind(dec_deg) + cosd(latitude_deg) * cosd(dec_deg) * cosd(ha);
    sin_alt.clamp(-1.0, 1.0).asin() * RAD
}

/// Combined observer sky for one frame.
#[derive(Debug, Clone)]
pub struct SkyView {
    pub sun: SunState,
    pub weather: SkyWeather,
    /// Solstice-phased season in `[0, 1)`: 0 local winter solstice, 0.5 local summer.
    /// Southern latitudes are flipped so 0 is still local winter.
    pub season: f32,
}

impl SkyView {
    pub fn at(
        latitude_deg: f64,
        longitude_deg: f64,
        utc: DateTime<Utc>,
        weather: SkyWeather,
    ) -> Self {
        Self {
            sun: SunState::at(latitude_deg, longitude_deg, utc),
            weather,
            season: season_from_utc(utc, latitude_deg),
        }
    }

    pub fn now(latitude_deg: f64, longitude_deg: f64, weather: SkyWeather) -> Self {
        Self::at(latitude_deg, longitude_deg, Utc::now(), weather)
    }
}

fn northern_winter_solstice(year: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, 12, 21).expect("solstice date")
}

fn hemisphere_season(s: f64, latitude_deg: f64) -> f64 {
    let s = s.rem_euclid(1.0);
    if latitude_deg < 0.0 {
        (s + 0.5).rem_euclid(1.0)
    } else {
        s
    }
}

/// Local season in `[0, 1)` from UTC and observer latitude.
/// 0 = local winter solstice, 0.25 spring, 0.5 summer, 0.75 autumn.
pub fn season_from_utc(utc: DateTime<Utc>, latitude_deg: f64) -> f32 {
    let date = utc.date_naive();
    let this = northern_winter_solstice(date.year());
    let origin = if date >= this {
        this
    } else {
        northern_winter_solstice(date.year() - 1)
    };
    hemisphere_season((date - origin).num_days() as f64 / 365.25, latitude_deg) as f32
}

/// Civil date for `season` at `latitude_deg`.
/// Origin is 2024-12-21 (northern winter solstice); offset is `round(season * 365.25)` days.
pub fn date_for_season(season: f32, latitude_deg: f64) -> NaiveDate {
    let s = hemisphere_season(f64::from(season), latitude_deg);
    northern_winter_solstice(2024) + Duration::days((s * 365.25).round() as i64)
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

    #[test]
    fn sun_dir_2d_is_altitude_only() {
        let horizon = sun_dir_2d(0.0);
        assert!((horizon[0] - 1.0).abs() < 1e-5);
        assert!(horizon[1].abs() < 1e-5);
        assert_eq!(horizon[2], 0.0);
        let zenith = sun_dir_2d(90.0);
        assert!(zenith[0].abs() < 1e-5);
        assert!((zenith[1] - 1.0).abs() < 1e-5);
        assert_eq!(zenith[2], 0.0);
    }

    fn near(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() < eps, "{a} ≉ {b}");
    }

    #[test]
    fn beijing_solstices_are_season_poles() {
        let summer = Utc.with_ymd_and_hms(2024, 6, 21, 4, 0, 0).unwrap();
        let winter = Utc.with_ymd_and_hms(2024, 12, 21, 4, 0, 0).unwrap();
        near(season_from_utc(summer, BEIJING.0), 0.5, 0.02);
        near(season_from_utc(winter, BEIJING.0), 0.0, 0.02);
    }

    #[test]
    fn southern_latitude_flips_season() {
        let june = Utc.with_ymd_and_hms(2024, 6, 21, 2, 0, 0).unwrap();
        near(season_from_utc(june, -33.87), 0.0, 0.02);
        near(season_from_utc(june, 33.87), 0.5, 0.02);
    }

    fn season_wrap_dist(a: f32, b: f32) -> f32 {
        let d = (a - b).abs();
        d.min(1.0 - d)
    }

    #[test]
    fn date_for_season_roundtrips_solstices() {
        for &(s, lat) in &[
            (0.0, BEIJING.0),
            (0.25, BEIJING.0),
            (0.5, BEIJING.0),
            (0.75, BEIJING.0),
            (0.0, -33.87),
            (0.25, -33.87),
            (0.5, -33.87),
            (0.75, -33.87),
        ] {
            let date = date_for_season(s, lat);
            let utc = date
                .and_hms_opt(12, 0, 0)
                .expect("noon")
                .and_utc();
            let got = season_from_utc(utc, lat);
            assert!(
                season_wrap_dist(got, s) < 0.02,
                "season {s} lat {lat}: date {date} -> {got}"
            );
        }
        let summer = date_for_season(0.5, BEIJING.0);
        assert_eq!(summer.month(), 6);
        let sydney_summer = date_for_season(0.5, -33.87);
        assert_eq!(sydney_summer.month(), 12);
    }
}
