//! Shared alt-az polar projection (N up), matching Daily sky map.

use std::f64::consts::TAU;

/// Plot/sky radius at the horizon (degrees of altitude from zenith).
pub const HORIZON_R: f64 = 90.0;

/// Convert azimuth/altitude (degrees) to polar plot XY.
/// Convention: zenith at origin, horizon at `r = 90`, N = +Y, E = −X.
pub fn az_alt_to_xy(az_deg: f64, alt_deg: f64) -> [f64; 2] {
    let az_rad = (az_deg + 90.0).to_radians();
    let r = (HORIZON_R - alt_deg).max(0.0);
    [r * az_rad.cos(), r * az_rad.sin()]
}

/// Inverse of [`az_alt_to_xy`]. Altitude is clamped to `[0, 90]`.
pub fn xy_to_az_alt(x: f64, y: f64) -> (f64, f64) {
    let r = (x * x + y * y).sqrt();
    let alt = (HORIZON_R - r).clamp(0.0, 90.0);
    // atan2(y, x) with our θ = az+90 → az = atan2(y,x) in degrees − 90
    let mut az = y.atan2(x).to_degrees() - 90.0;
    az = az.rem_euclid(360.0);
    (az, alt)
}

/// Sample points along an altitude arc from `az_start` toward `az_end` (degrees).
/// If `wrap`, the arc goes the long way across north (`az_start → 360/0 → az_end`).
pub fn arc_points(
    az_start: f64,
    az_end: f64,
    alt_deg: f64,
    wrap: bool,
    steps: usize,
) -> Vec<[f64; 2]> {
    let span = if wrap {
        (360.0 - az_start) + az_end
    } else {
        az_end - az_start
    };
    let n = steps.max(2);
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let az = (az_start + span * t).rem_euclid(360.0);
            az_alt_to_xy(az, alt_deg)
        })
        .collect()
}

/// Full circle polyline at a given plot radius.
pub fn circle_points(r: f64, steps: usize) -> Vec<[f64; 2]> {
    let n = steps.max(8);
    (0..=n)
        .map(|i| {
            let t = TAU * (i as f64) / (n as f64);
            [r * t.cos(), r * t.sin()]
        })
        .collect()
}
