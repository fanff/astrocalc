//! ISS sunlight (Earth umbra) and observer sky darkness.

use astro::{angle, ecliptic, sun, time};
use chrono::{DateTime, Utc};
use julian_day_converter::unixtime_to_julian_day;

use crate::solarsystemcalc::{ecl_point_to_radec, sph_to_cart};

use super::propagate::{EARTH_RADIUS_MEAN_KM, Observer, teme_to_ecef};

/// Default: nautical twilight — Sun below −6°.
pub const DEFAULT_OBSERVER_SUN_MAX_ALT_DEG: f64 = -6.0;

/// Unit Sun direction in ECEF at `utc` (from Earth toward Sun).
pub fn sun_ecef_unit(utc: DateTime<Utc>) -> [f64; 3] {
    let jd = unixtime_to_julian_day(utc.timestamp());
    let eps = ecliptic::mn_oblq_IAU(jd);
    let (sun_ecl, _) = sun::geocent_ecl_pos(jd);
    let (ra, dec) = ecl_point_to_radec(&sun_ecl, eps);
    // Equatorial unit vector (approx TEME/True-of-date for visual work).
    let (x, y, z) = sph_to_cart(ra, dec, 1.0);
    teme_to_ecef([x, y, z], utc)
}

/// Geocentric solar altitude (degrees) for the observer (no refraction).
pub fn observer_sun_altitude_deg(observer: &Observer, utc: DateTime<Utc>) -> f64 {
    let jd = unixtime_to_julian_day(utc.timestamp());
    let gmst = time::mn_sidr(jd);
    let eps = ecliptic::mn_oblq_IAU(jd);
    let (sun_ecl, _) = sun::geocent_ecl_pos(jd);
    let (ra, dec) = ecl_point_to_radec(&sun_ecl, eps);
    let lat_rad = observer.lat_deg.to_radians();
    let lon_rad = observer.lon_deg.to_radians();
    let hour_angle = coords_hour_angle(gmst, lon_rad, ra);
    let (az, alt) = astro::loc_hz_frm_eq!(hour_angle, dec, lat_rad);
    let _ = az;
    alt.to_degrees()
}

fn coords_hour_angle(gmst: f64, lon_rad: f64, ra: f64) -> f64 {
    astro::coords::hr_angl_frm_observer_long(gmst, -lon_rad, ra)
}

/// True when the Sun is deep enough below the horizon for a dark sky.
pub fn observer_sky_dark(observer: &Observer, utc: DateTime<Utc>, max_sun_alt_deg: f64) -> bool {
    observer_sun_altitude_deg(observer, utc) < max_sun_alt_deg
}

/// Cylindrical Earth umbra: satellite sunlit if not in the night-side cylinder.
pub fn satellite_sunlit(sat_ecef_km: [f64; 3], utc: DateTime<Utc>) -> bool {
    let sun = sun_ecef_unit(utc);
    // Projection of sat onto Sun axis; negative ⇒ night side of Earth.
    let along = sat_ecef_km[0] * sun[0] + sat_ecef_km[1] * sun[1] + sat_ecef_km[2] * sun[2];
    if along > 0.0 {
        return true; // day side of Earth
    }
    let cross0 = sat_ecef_km[1] * sun[2] - sat_ecef_km[2] * sun[1];
    let cross1 = sat_ecef_km[2] * sun[0] - sat_ecef_km[0] * sun[2];
    let cross2 = sat_ecef_km[0] * sun[1] - sat_ecef_km[1] * sun[0];
    let radial = (cross0 * cross0 + cross1 * cross1 + cross2 * cross2).sqrt();
    radial > EARTH_RADIUS_MEAN_KM
}

/// Visible-pass illumination: ISS sunlit and observer sky dark.
pub fn pass_illumination_ok(
    observer: &Observer,
    sat_ecef_km: [f64; 3],
    utc: DateTime<Utc>,
    max_sun_alt_deg: f64,
) -> bool {
    satellite_sunlit(sat_ecef_km, utc) && observer_sky_dark(observer, utc, max_sun_alt_deg)
}

/// Apparent solar angular semi-diameter (degrees) from geocentric distance.
pub fn sun_semi_diameter_deg(utc: DateTime<Utc>) -> f64 {
    let jd = unixtime_to_julian_day(utc.timestamp());
    let (_ecl, au) = sun::geocent_ecl_pos(jd);
    // Solar radius ≈ 695_700 km; 1 AU ≈ 149_597_870.7 km
    let dist_km = au * 149_597_870.7;
    (695_700.0 / dist_km).asin().to_degrees()
}

/// Moon angular semi-diameter (degrees) from Earth–Moon distance (km).
pub fn moon_semi_diameter_deg(earth_moon_km: f64) -> f64 {
    const MOON_RADIUS_KM: f64 = 1737.4;
    (MOON_RADIUS_KM / earth_moon_km).asin().to_degrees()
}

/// Unit vector from RA/Dec (radians).
pub fn radec_unit(ra_rad: f64, dec_rad: f64) -> [f64; 3] {
    let (x, y, z) = sph_to_cart(ra_rad, dec_rad, 1.0);
    [x, y, z]
}

/// Angular separation between two unit vectors (degrees).
pub fn ang_sep_deg(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
    dot.acos().to_degrees()
}

/// Sun RA/Dec (radians) at `utc`.
pub fn sun_radec_rad(utc: DateTime<Utc>) -> (f64, f64) {
    let jd = unixtime_to_julian_day(utc.timestamp());
    let eps = ecliptic::mn_oblq_IAU(jd);
    let (sun_ecl, _) = sun::geocent_ecl_pos(jd);
    ecl_point_to_radec(&sun_ecl, eps)
}

/// Moon RA/Dec (radians) and distance (km) at `utc`.
pub fn moon_radec_dist(utc: DateTime<Utc>) -> (f64, f64, f64) {
    let jd = unixtime_to_julian_day(utc.timestamp());
    let eps = ecliptic::mn_oblq_IAU(jd);
    let (moon_ecl, dist) = astro::lunar::geocent_ecl_pos(jd);
    let (ra, dec) = ecl_point_to_radec(&moon_ecl, eps);
    (ra, dec, dist)
}

/// Moon illuminated fraction as percent `[0, 100]`.
pub fn moon_illum_pct(utc: DateTime<Utc>) -> f64 {
    let jd = unixtime_to_julian_day(utc.timestamp());
    let (moon_ecl, earth_moon_dist) = astro::lunar::geocent_ecl_pos(jd);
    let (sun_ecl, sun_earth_au) = sun::geocent_ecl_pos(jd);
    let fract = astro::lunar::illum_frac_frm_ecl_coords(
        moon_ecl.long,
        moon_ecl.lat,
        sun_ecl.long,
        earth_moon_dist,
        sun_earth_au * 149_597_870.7,
    );
    (fract * 100.0).clamp(0.0, 100.0)
}

/// Short phase name + percent for UI (ASCII-safe).
pub fn moon_phase_label(illum_pct: f64) -> String {
    let pct = illum_pct.clamp(0.0, 100.0);
    let name = match pct {
        x if x < 5.0 => "New",
        x if x < 45.0 => "Crescent",
        x if x < 55.0 => "Quarter",
        x if x < 95.0 => "Gibbous",
        _ => "Full",
    };
    format!("{name} ({pct:.0}%)")
}

/// Geocentric altitude/azimuth of the Sun (north-based az), matching ObjectPosition.
pub fn sun_alt_az(observer: &Observer, utc: DateTime<Utc>) -> (f64, f64) {
    let jd = unixtime_to_julian_day(utc.timestamp());
    let gmst = time::mn_sidr(jd);
    let eps = ecliptic::mn_oblq_IAU(jd);
    let (sun_ecl, _) = sun::geocent_ecl_pos(jd);
    let (ra, dec) = ecl_point_to_radec(&sun_ecl, eps);
    let lat_rad = observer.lat_deg.to_radians();
    let lon_rad = observer.lon_deg.to_radians();
    let hour_angle = coords_hour_angle(gmst, lon_rad, ra);
    let (az, alt) = astro::loc_hz_frm_eq!(hour_angle, dec, lat_rad);
    (
        alt.to_degrees(),
        angle::limit_to_360(az.to_degrees() + 180.0),
    )
}

/// Geocentric lunar altitude (degrees) for the observer (no refraction).
pub fn observer_moon_altitude_deg(observer: &Observer, utc: DateTime<Utc>) -> f64 {
    let jd = unixtime_to_julian_day(utc.timestamp());
    let gmst = time::mn_sidr(jd);
    let (ra, dec, _) = moon_radec_dist(utc);
    let lat_rad = observer.lat_deg.to_radians();
    let lon_rad = observer.lon_deg.to_radians();
    let hour_angle = coords_hour_angle(gmst, lon_rad, ra);
    let (_az, alt) = astro::loc_hz_frm_eq!(hour_angle, dec, lat_rad);
    alt.to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn noon_paris_sun_above_horizon_june() {
        let obs = Observer::new(48.8566, 2.3522);
        let utc = Utc.with_ymd_and_hms(2006, 6, 16, 12, 0, 0).unwrap();
        let alt = observer_sun_altitude_deg(&obs, utc);
        assert!(alt > 20.0, "sun alt at noon Paris June: {alt}");
        assert!(!observer_sky_dark(
            &obs,
            utc,
            DEFAULT_OBSERVER_SUN_MAX_ALT_DEG
        ));
    }

    #[test]
    fn midnight_paris_sky_dark_june() {
        let obs = Observer::new(48.8566, 2.3522);
        let utc = Utc.with_ymd_and_hms(2006, 6, 16, 0, 0, 0).unwrap();
        let alt = observer_sun_altitude_deg(&obs, utc);
        assert!(alt < -6.0, "sun alt midnight: {alt}");
        assert!(observer_sky_dark(
            &obs,
            utc,
            DEFAULT_OBSERVER_SUN_MAX_ALT_DEG
        ));
    }

    #[test]
    fn umbra_day_side_is_sunlit() {
        let utc = Utc.with_ymd_and_hms(2006, 6, 16, 12, 0, 0).unwrap();
        let sun = sun_ecef_unit(utc);
        // Point on day side outside Earth.
        let sat = [sun[0] * 7000.0, sun[1] * 7000.0, sun[2] * 7000.0];
        assert!(satellite_sunlit(sat, utc));
    }

    #[test]
    fn umbra_night_cylinder_is_dark() {
        let utc = Utc.with_ymd_and_hms(2006, 6, 16, 12, 0, 0).unwrap();
        let sun = sun_ecef_unit(utc);
        // Directly anti-sun at LEO altitude → in umbra.
        let sat = [-sun[0] * 7000.0, -sun[1] * 7000.0, -sun[2] * 7000.0];
        assert!(!satellite_sunlit(sat, utc));
    }

    #[test]
    fn sun_semi_diameter_reasonable() {
        let utc = Utc.with_ymd_and_hms(2006, 6, 16, 12, 0, 0).unwrap();
        let sd = sun_semi_diameter_deg(utc);
        assert!((0.25..0.28).contains(&sd), "semi-diameter {sd}°");
    }
}
