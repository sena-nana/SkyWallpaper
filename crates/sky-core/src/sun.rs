use chrono::{DateTime, Utc};

use crate::{cosd, equatorial_to_altaz, julian_centuries, julian_date, local_sidereal_deg, sind, wrap_deg};

/// Apparent solar position at the observer.
#[derive(Debug, Clone, Copy)]
pub struct SunState {
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
    pub dir_enu: [f32; 3],
}

impl SunState {
    pub fn at(latitude_deg: f64, longitude_deg: f64, utc: DateTime<Utc>) -> Self {
        let jd = julian_date(utc);
        let t = julian_centuries(jd);

        let l0 = wrap_deg(280.46646 + 36000.76983 * t + 0.0003032 * t * t);
        let m = wrap_deg(357.52911 + 35999.05029 * t - 0.0001537 * t * t);
        let c = (1.914602 - 0.004817 * t - 0.000014 * t * t) * sind(m)
            + (0.019993 - 0.000101 * t) * sind(2.0 * m)
            + 0.000289 * sind(3.0 * m);
        let true_long = l0 + c;
        let omega = 125.04 - 1934.136 * t;
        let lambda = true_long - 0.00569 - 0.00478 * sind(omega);
        let eps0 = 23.439291 - 0.0130042 * t - 0.00000016 * t * t + 0.000000504 * t * t * t;
        let eps = eps0 + 0.00256 * cosd(omega);
        let ra = wrap_deg(
            (cosd(eps) * sind(lambda)).atan2(cosd(lambda)) * crate::RAD,
        );
        let dec = (sind(eps) * sind(lambda)).asin() * crate::RAD;

        let lst = local_sidereal_deg(jd, longitude_deg);
        let (altitude, azimuth) = equatorial_to_altaz(ra, dec, lst, latitude_deg);

        Self {
            altitude_deg: altitude,
            azimuth_deg: azimuth,
            dir_enu: crate::enu_from_alt_az(altitude, azimuth),
        }
    }
}
