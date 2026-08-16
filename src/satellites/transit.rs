//! ISS Sun / Moon disk transit and near-miss detection.

use chrono::{DateTime, Duration, Utc};

use crate::config::ViewWindow;

use super::events::{DiskBody, DiskTransit};
use super::illumination::{
    ang_sep_deg, moon_illum_pct, moon_radec_dist, moon_semi_diameter_deg, radec_unit, sun_alt_az,
    sun_radec_rad, sun_semi_diameter_deg,
};
use super::propagate::{Observer, Propagator, teme_to_ecef};

/// Skip Moon disk events when the lit fraction is below this (silhouette useless).
pub const MIN_MOON_ILLUM_PCT_FOR_DISK: f64 = 10.0;

pub struct TransitSearchParams {
    pub coarse_step: Duration,
    /// Only consider times when ISS altitude ≥ this (degrees).
    pub min_altitude_deg: f64,
    /// Coarse candidate if separation < this (degrees); ~3° catch radius.
    pub candidate_sep_deg: f64,
    /// Report near-misses up to this multiple of the semi-diameter.
    pub near_miss_factor: f64,
}

impl Default for TransitSearchParams {
    fn default() -> Self {
        Self {
            coarse_step: Duration::seconds(5),
            min_altitude_deg: 5.0,
            candidate_sep_deg: 3.0,
            near_miss_factor: 3.0,
        }
    }
}

/// Topocentric unit vector toward the satellite (from observer), in equatorial-ish TEME.
fn iss_look_unit(
    prop: &Propagator,
    observer: &Observer,
    utc: DateTime<Utc>,
) -> Result<[f64; 3], String> {
    let teme = prop.teme_km_at(utc)?;
    let sat = teme_to_ecef(teme, utc);
    let obs = observer.ecef_km();
    let mut v = [sat[0] - obs[0], sat[1] - obs[1], sat[2] - obs[2]];
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n < 1e-9 {
        return Err("degenerate range".into());
    }
    v[0] /= n;
    v[1] /= n;
    v[2] /= n;
    Ok(v)
}

fn body_look_unit(body: DiskBody, utc: DateTime<Utc>) -> ([f64; 3], f64) {
    match body {
        DiskBody::Sun => {
            let (ra, dec) = sun_radec_rad(utc);
            // Sun direction in TEME/True-of-date equatorial; rotate to ECEF for consistency
            // with ISS look vector (which is ECEF-based).
            let u = radec_unit(ra, dec);
            let ecef = teme_to_ecef(u, utc);
            let n = (ecef[0] * ecef[0] + ecef[1] * ecef[1] + ecef[2] * ecef[2]).sqrt();
            (
                [ecef[0] / n, ecef[1] / n, ecef[2] / n],
                sun_semi_diameter_deg(utc),
            )
        }
        DiskBody::Moon => {
            let (ra, dec, dist) = moon_radec_dist(utc);
            let u = radec_unit(ra, dec);
            let ecef = teme_to_ecef(u, utc);
            let n = (ecef[0] * ecef[0] + ecef[1] * ecef[1] + ecef[2] * ecef[2]).sqrt();
            (
                [ecef[0] / n, ecef[1] / n, ecef[2] / n],
                moon_semi_diameter_deg(dist),
            )
        }
    }
}

fn separation_deg(
    prop: &Propagator,
    observer: &Observer,
    body: DiskBody,
    utc: DateTime<Utc>,
) -> Result<(f64, f64), String> {
    let iss = iss_look_unit(prop, observer, utc)?;
    let (body_u, semi) = body_look_unit(body, utc);
    Ok((ang_sep_deg(iss, body_u), semi))
}

/// Estimate ground move (km + bearing from north) that reduces angular miss.
/// Uses a local gradient of separation vs observer lat/lon.
fn move_hint_toward_centerline(
    prop: &Propagator,
    observer: &Observer,
    body: DiskBody,
    utc: DateTime<Utc>,
    sep_deg: f64,
) -> Option<(f64, f64)> {
    if sep_deg < 1e-6 {
        return None;
    }
    let lat = observer.lat_deg;
    let lon = observer.lon_deg;
    let d_deg = 0.01; // ~1.1 km
    let cos_lat = lat.to_radians().cos().abs().max(0.2);
    let obs_n = Observer::new(lat + d_deg, lon);
    let obs_e = Observer::new(lat, lon + d_deg / cos_lat);
    let (sep0, _) = separation_deg(prop, observer, body, utc).ok()?;
    let (sep_n, _) = separation_deg(prop, &obs_n, body, utc).ok()?;
    let (sep_e, _) = separation_deg(prop, &obs_e, body, utc).ok()?;

    let km_per_deg_n = 111.32;
    let km_per_deg_e = 111.32 * cos_lat;
    let dsep_dn = (sep_n - sep0) / (d_deg * km_per_deg_n); // deg / km north
    let dsep_de = (sep_e - sep0) / ((d_deg / cos_lat) * km_per_deg_e); // deg / km east
    let grad_mag = (dsep_dn * dsep_dn + dsep_de * dsep_de).sqrt();
    if grad_mag < 1e-9 {
        return None;
    }
    // Move opposite the gradient to decrease separation.
    let north = -dsep_dn / grad_mag;
    let east = -dsep_de / grad_mag;
    let mut bearing = east.atan2(north).to_degrees();
    if bearing < 0.0 {
        bearing += 360.0;
    }
    let dist_km = (sep0 / grad_mag).clamp(0.05, 500.0);
    Some((dist_km, bearing))
}

/// Compass octant label for a bearing (degrees from north toward east).
pub fn bearing_to_compass(bearing_deg: f64) -> &'static str {
    let b = ((bearing_deg % 360.0) + 360.0) % 360.0;
    let idx = ((b + 22.5) / 45.0).floor() as i32 % 8;
    match idx {
        0 => "north",
        1 => "north-east",
        2 => "east",
        3 => "south-east",
        4 => "south",
        5 => "south-west",
        6 => "west",
        _ => "north-west",
    }
}

/// Bisect to local minimum of angular separation in `[lo, hi]`.
fn refine_min_sep(
    prop: &Propagator,
    observer: &Observer,
    body: DiskBody,
    mut lo: DateTime<Utc>,
    mut hi: DateTime<Utc>,
) -> Result<(DateTime<Utc>, f64, f64), String> {
    for _ in 0..40 {
        if (hi - lo) <= Duration::milliseconds(50) {
            break;
        }
        let m1 = lo + (hi - lo) / 3;
        let m2 = hi - (hi - lo) / 3;
        let (s1, _) = separation_deg(prop, observer, body, m1)?;
        let (s2, _) = separation_deg(prop, observer, body, m2)?;
        if s1 < s2 {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let mid = lo + (hi - lo) / 2;
    let (sep, semi) = separation_deg(prop, observer, body, mid)?;
    Ok((mid, sep, semi))
}

fn body_alt_az(body: DiskBody, observer: &Observer, utc: DateTime<Utc>) -> (f64, f64) {
    match body {
        DiskBody::Sun => sun_alt_az(observer, utc),
        DiskBody::Moon => {
            // Reuse moon_position path via RA/Dec hour angle would duplicate; use sun_alt_az style.
            let jd = julian_day_converter::unixtime_to_julian_day(utc.timestamp());
            let gmst = astro::time::mn_sidr(jd);
            let eps = astro::ecliptic::mn_oblq_IAU(jd);
            let (moon_ecl, _) = astro::lunar::geocent_ecl_pos(jd);
            let (ra, dec) = crate::solarsystemcalc::ecl_point_to_radec(&moon_ecl, eps);
            let lat_rad = observer.lat_deg.to_radians();
            let lon_rad = observer.lon_deg.to_radians();
            let ha = astro::coords::hr_angl_frm_observer_long(gmst, -lon_rad, ra);
            let (az, alt) = astro::loc_hz_frm_eq!(ha, dec, lat_rad);
            (
                alt.to_degrees(),
                astro::angle::limit_to_360(az.to_degrees() + 180.0),
            )
        }
    }
}

/// Scan for disk transits / near-misses of `body` in `[start, end]`.
pub fn find_disk_events(
    prop: &Propagator,
    observer: &Observer,
    body: DiskBody,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    params: &TransitSearchParams,
    view_windows: &[ViewWindow],
) -> Result<Vec<DiskTransit>, String> {
    if end <= start {
        return Ok(vec![]);
    }

    let mut samples: Vec<(DateTime<Utc>, f64, f64, f64)> = Vec::new(); // t, sep, semi, alt
    let mut t = start;
    while t <= end {
        let topo = prop.observe(observer, t)?;
        if topo.altitude_deg >= params.min_altitude_deg {
            let (sep, semi) = separation_deg(prop, observer, body, t)?;
            samples.push((t, sep, semi, topo.altitude_deg));
        }
        t += params.coarse_step;
    }

    let mut events = Vec::new();
    let mut i = 1;
    while i + 1 < samples.len() {
        let (t0, s0, _, _) = samples[i - 1];
        let (t1, s1, semi1, _) = samples[i];
        let (t2, s2, _, _) = samples[i + 1];
        // Local minimum in separation.
        if s1 <= s0 && s1 <= s2 && s1 < params.candidate_sep_deg {
            let (center, sep, semi) = refine_min_sep(prop, observer, body, t0, t2)?;
            let topo = prop.observe(observer, center)?;
            if topo.altitude_deg < params.min_altitude_deg {
                i += 1;
                continue;
            }
            let is_transit = sep <= semi;
            let near_ok = sep <= semi * params.near_miss_factor;
            if !is_transit && !near_ok {
                i += 1;
                continue;
            }

            let (body_alt, body_az) = body_alt_az(body, observer, center);
            if !view_windows.is_empty() {
                let ok = view_windows
                    .iter()
                    .any(|vw| vw.contains(body_az, body_alt.max(0.0)));
                if !ok {
                    i += 1;
                    continue;
                }
            }

            // Rough miss distance: sep (rad) * slant range.
            let miss_km = Some(sep.to_radians() * topo.range_km);
            let (move_km, move_bearing) = if is_transit {
                (None, None)
            } else {
                match move_hint_toward_centerline(prop, observer, body, center, sep) {
                    Some((km, brg)) => (Some(km), Some(brg)),
                    None => (miss_km, None),
                }
            };

            let moon_illum = if body == DiskBody::Moon {
                let pct = moon_illum_pct(center);
                if pct < MIN_MOON_ILLUM_PCT_FOR_DISK {
                    i += 1;
                    continue;
                }
                Some(pct)
            } else {
                None
            };

            events.push(DiskTransit {
                body,
                center_time: center,
                min_separation_deg: sep,
                semi_diameter_deg: semi,
                is_transit,
                altitude_deg: topo.altitude_deg,
                azimuth_deg: topo.azimuth_deg,
                range_km: topo.range_km,
                centerline_miss_km: miss_km,
                move_hint_km: move_km,
                move_hint_bearing_deg: move_bearing,
                moon_illum_pct: moon_illum,
            });

            // Skip ahead past this trough.
            let _ = (t1, semi1);
            i += 3;
            continue;
        }
        i += 1;
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::satellites::fixtures;

    #[test]
    fn separation_is_finite_and_bisect_monotonic() {
        let prop =
            Propagator::from_tle(Some("ISS"), fixtures::ISS_TLE_L1, fixtures::ISS_TLE_L2).unwrap();
        let obs = Observer::new(48.8566, 2.3522);
        let t0 = prop.epoch_utc();
        let (s0, semi) = separation_deg(&prop, &obs, DiskBody::Sun, t0).unwrap();
        assert!(s0.is_finite() && s0 >= 0.0 && s0 <= 180.0);
        assert!(semi > 0.2 && semi < 0.3);

        // Ternary search over a short window returns something ≤ coarse samples.
        let (center, sep, _) =
            refine_min_sep(&prop, &obs, DiskBody::Sun, t0, t0 + Duration::minutes(10)).unwrap();
        assert!(center >= t0 && center <= t0 + Duration::minutes(10));
        let (s_lo, _) = separation_deg(&prop, &obs, DiskBody::Sun, t0).unwrap();
        let (s_hi, _) =
            separation_deg(&prop, &obs, DiskBody::Sun, t0 + Duration::minutes(10)).unwrap();
        assert!(sep <= s_lo.max(s_hi) + 1e-6);
    }

    #[test]
    fn scan_runs_without_panic() {
        let prop =
            Propagator::from_tle(Some("ISS"), fixtures::ISS_TLE_L1, fixtures::ISS_TLE_L2).unwrap();
        let obs = Observer::new(48.8566, 2.3522);
        let start = prop.epoch_utc();
        let end = start + Duration::hours(6);
        let sun_ev = find_disk_events(
            &prop,
            &obs,
            DiskBody::Sun,
            start,
            end,
            &TransitSearchParams::default(),
            &[],
        )
        .unwrap();
        let moon_ev = find_disk_events(
            &prop,
            &obs,
            DiskBody::Moon,
            start,
            end,
            &TransitSearchParams::default(),
            &[],
        )
        .unwrap();
        // May be empty; ensure API works.
        for e in sun_ev.iter().chain(moon_ev.iter()) {
            assert!(e.min_separation_deg >= 0.0);
            assert!(e.semi_diameter_deg > 0.0);
            if e.is_transit {
                assert!(e.min_separation_deg <= e.semi_diameter_deg);
            }
        }
    }
}
