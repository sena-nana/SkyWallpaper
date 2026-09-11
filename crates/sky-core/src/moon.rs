use chrono::{DateTime, Utc};

use crate::{
    equatorial_to_altaz, julian_centuries, julian_date, local_sidereal_deg, sind, wrap_deg,
};

/// Apparent lunar position and phase.
#[derive(Debug, Clone, Copy)]
pub struct MoonState {
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
    /// 0 = new, 1 = full.
    pub illumination: f64,
    pub dir_enu: [f32; 3],
}

impl MoonState {
    pub fn at(latitude_deg: f64, longitude_deg: f64, utc: DateTime<Utc>) -> Self {
        let jd = julian_date(utc);
        let t = julian_centuries(jd);

        let lp = wrap_deg(218.3164477 + 481_267.88123421 * t);
        let d = wrap_deg(297.8501921 + 445_267.1114034 * t);
        let m = wrap_deg(357.5291092 + 35_999.0502909 * t);
        let mp = wrap_deg(134.9633964 + 477_198.8678671 * t);
        let f = wrap_deg(93.2720950 + 483_202.0175233 * t);

        let lon = lp
            + 6.289 * sind(mp)
            + 1.274 * sind(2.0 * d - mp)
            + 0.658 * sind(2.0 * d)
            + 0.214 * sind(2.0 * mp)
            - 0.186 * sind(m)
            - 0.114 * sind(2.0 * f)
            + 0.059 * sind(2.0 * d - 2.0 * mp)
            + 0.057 * sind(2.0 * d - m - mp)
            + 0.053 * sind(2.0 * d + mp)
            + 0.046 * sind(2.0 * d - m)
            + 0.041 * sind(mp - m)
            - 0.035 * sind(d)
            - 0.031 * sind(mp + m);

        let lat = 5.128 * sind(f)
            + 0.280 * sind(mp + f)
            + 0.277 * sind(mp - f)
            + 0.173 * sind(2.0 * d - f)
            + 0.055 * sind(2.0 * d - mp + f)
            + 0.046 * sind(2.0 * d - mp - f);

        let sun_lon = wrap_deg(280.46646 + 36_000.76983 * t);
        let elongation = wrap_deg(lon - sun_lon);
        let illumination = ((1.0 - crate::cosd(elongation)) * 0.5).clamp(0.0, 1.0);

        let eps = 23.439291 - 0.0130042 * t;
        let ecl_lon = lon * crate::DEG;
        let ecl_lat = lat * crate::DEG;
        let eps_r = eps * crate::DEG;
        let x = ecl_lat.cos() * ecl_lon.cos();
        let y = ecl_lat.cos() * ecl_lon.sin() * eps_r.cos() - ecl_lat.sin() * eps_r.sin();
        let z = ecl_lat.cos() * ecl_lon.sin() * eps_r.sin() + ecl_lat.sin() * eps_r.cos();
        let ra = wrap_deg(y.atan2(x) * crate::RAD);
        let dec = z.clamp(-1.0, 1.0).asin() * crate::RAD;

        let lst = local_sidereal_deg(jd, longitude_deg);
        let (altitude, azimuth) = equatorial_to_altaz(ra, dec, lst, latitude_deg);

        Self {
            altitude_deg: altitude,
            azimuth_deg: azimuth,
            illumination,
            dir_enu: crate::enu_from_alt_az(altitude, azimuth),
        }
    }
}
