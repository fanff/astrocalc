//! ISS apparent brightness: phase angle, magnitude, airmass extinction.

use chrono::{DateTime, Utc};

use super::illumination::sun_ecef_unit;
use super::propagate::Observer;

/// Standard magnitude of the ISS at 1000 km range and zero phase (approx.).
pub const ISS_STD_MAG_AT_1000KM: f64 = -1.3;

/// Visual extinction coefficient (mag per airmass), clear sky.
pub const EXTINCTION_K_MAG: f64 = 0.20;

/// Minimum useful visible-window duration (seconds).
pub const MIN_VISIBLE_DURATION_SECS: i64 = 20;

fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    [v[0] / n, v[1] / n, v[2] / n]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Phase angle at the ISS (degrees): Sun–ISS–observer.
///
/// 0° = fully face-lit toward the observer; 180° = back-lit.
pub fn phase_angle_deg(sat_ecef_km: [f64; 3], observer: &Observer, utc: DateTime<Utc>) -> f64 {
    let sun = sun_ecef_unit(utc);
    // Sun direction from ISS ≈ geocentric sun (Sun is far).
    let to_sun = sun;
    let obs = observer.ecef_km();
    let to_obs = unit([
        obs[0] - sat_ecef_km[0],
        obs[1] - sat_ecef_km[1],
        obs[2] - sat_ecef_km[2],
    ]);
    let c = dot(to_sun, to_obs).clamp(-1.0, 1.0);
    c.acos().to_degrees()
}

/// Relative optical airmass from altitude (degrees). Secant formula with horizon floor.
pub fn airmass(altitude_deg: f64) -> f64 {
    let alt = altitude_deg.max(0.5);
    let z = (90.0 - alt).to_radians();
    // Hardie-like simple: X = 1 / (cos z + 0.025 * exp(-11 * cos z)) — use Kasten-like lite
    let cos_z = z.cos().max(0.01);
    1.0 / (cos_z + 0.025 * (-11.0 * cos_z).exp())
}

/// Phase function contribution (magnitudes): dimmer as phase approaches 180°.
fn phase_term_mag(phase_deg: f64) -> f64 {
    let phi = phase_deg.to_radians();
    // Diffuse sphere-ish: -2.5 log10((1+cos φ)/2)
    let lit = ((1.0 + phi.cos()) * 0.5).clamp(1e-4, 1.0);
    -2.5 * lit.log10()
}

/// Apparent magnitude from range (km), phase angle (deg), and altitude (deg).
pub fn apparent_magnitude(range_km: f64, phase_deg: f64, altitude_deg: f64) -> f64 {
    let range = range_km.max(200.0);
    let dist_term = 5.0 * (range / 1000.0).log10();
    let m = ISS_STD_MAG_AT_1000KM
        + dist_term
        + phase_term_mag(phase_deg)
        + EXTINCTION_K_MAG * (airmass(altitude_deg) - 1.0);
    m
}

/// Short brightness phrase for UI (ASCII).
pub fn magnitude_phrase(mag: f64) -> &'static str {
    if mag < -2.0 {
        "bright"
    } else if mag < 0.5 {
        "easy"
    } else if mag < 2.0 {
        "modest"
    } else {
        "faint"
    }
}

/// Naked-eye chance given magnitude and Bortle class (1=dark … 9=inner city).
pub fn naked_eye_label(mag: f64, bortle: u8) -> &'static str {
    let b = bortle.clamp(1, 9);
    // Rough limiting mag for naked eye under that Bortle (very approximate).
    let limit = match b {
        1 => 7.0,
        2 => 6.5,
        3 => 6.0,
        4 => 5.5,
        5 => 5.0,
        6 => 4.5,
        7 => 4.0,
        8 => 3.5,
        _ => 3.0,
    };
    if mag <= limit - 2.5 {
        "good naked-eye chance"
    } else if mag <= limit - 0.5 {
        "possible naked-eye"
    } else {
        "challenging from this sky"
    }
}

/// Cloud cover % -> short label.
pub fn cloud_label(cloud_pct: Option<f64>) -> &'static str {
    match cloud_pct {
        None => "clouds unknown",
        Some(c) if c < 25.0 => "clear",
        Some(c) if c < 60.0 => "partly cloudy",
        Some(_) => "cloudy",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn phase_angle_in_0_180() {
        let obs = Observer::new(48.8566, 2.3522);
        let utc = Utc.with_ymd_and_hms(2008, 9, 20, 12, 0, 0).unwrap();
        // Arbitrary LEO-ish ECEF
        let sat = [4000.0, 3000.0, 3000.0];
        let ph = phase_angle_deg(sat, &obs, utc);
        assert!((0.0..=180.0).contains(&ph), "phase={ph}");
    }

    #[test]
    fn closer_is_brighter() {
        let m_near = apparent_magnitude(400.0, 40.0, 60.0);
        let m_far = apparent_magnitude(1200.0, 40.0, 60.0);
        assert!(m_near < m_far, "near={m_near} far={m_far}");
    }

    #[test]
    fn larger_phase_is_dimmer() {
        let m0 = apparent_magnitude(600.0, 20.0, 50.0);
        let m1 = apparent_magnitude(600.0, 120.0, 50.0);
        assert!(m0 < m1, "face={m0} back={m1}");
    }

    #[test]
    fn horizon_dimmer_than_zenith() {
        let m_high = apparent_magnitude(500.0, 50.0, 80.0);
        let m_low = apparent_magnitude(500.0, 50.0, 10.0);
        assert!(m_high < m_low, "high={m_high} low={m_low}");
    }

    #[test]
    fn phrases_and_cloud() {
        assert_eq!(magnitude_phrase(-3.0), "bright");
        assert_eq!(cloud_label(Some(10.0)), "clear");
        assert_eq!(cloud_label(Some(80.0)), "cloudy");
        assert_eq!(naked_eye_label(-2.0, 4), "good naked-eye chance");
    }
}
