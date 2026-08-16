//! Detect sunlit ISS passes over an observer / view windows.

use chrono::{DateTime, Duration, Utc};

use crate::config::ViewWindow;

use super::brightness::{MIN_VISIBLE_DURATION_SECS, apparent_magnitude, phase_angle_deg};
use super::events::{TrackSample, VisiblePass};
use super::illumination::{DEFAULT_OBSERVER_SUN_MAX_ALT_DEG, pass_illumination_ok};
use super::propagate::{Observer, Propagator, teme_to_ecef};

fn sat_ecef(prop: &Propagator, utc: DateTime<Utc>) -> Result<[f64; 3], String> {
    let teme = prop.teme_km_at(utc)?;
    Ok(teme_to_ecef(teme, utc))
}

pub struct PassSearchParams {
    pub min_altitude_deg: f64,
    pub coarse_step: Duration,
    pub fine_step: Duration,
    pub sample_step: Duration,
    pub max_sun_alt_deg: f64,
    /// If true, listed AOS/LOS is the sunlit + dark-sky window (not full geometric pass).
    pub require_illumination: bool,
}

impl Default for PassSearchParams {
    fn default() -> Self {
        Self {
            min_altitude_deg: 10.0,
            coarse_step: Duration::seconds(30),
            fine_step: Duration::seconds(1),
            sample_step: Duration::seconds(5),
            max_sun_alt_deg: DEFAULT_OBSERVER_SUN_MAX_ALT_DEG,
            require_illumination: true,
        }
    }
}

/// Find ISS passes between `start` and `end` (UTC).
pub fn find_passes(
    prop: &Propagator,
    observer: &Observer,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    params: &PassSearchParams,
    view_windows: &[ViewWindow],
) -> Result<Vec<VisiblePass>, String> {
    if end <= start {
        return Ok(vec![]);
    }

    let mut alts: Vec<(DateTime<Utc>, f64)> = Vec::new();
    let mut t = start;
    while t <= end {
        let topo = prop.observe(observer, t)?;
        alts.push((t, topo.altitude_deg));
        t += params.coarse_step;
    }
    if alts.is_empty() {
        return Ok(vec![]);
    }

    let mut passes = Vec::new();
    let mut i = 0;
    while i + 1 < alts.len() {
        // Rising edge across min altitude.
        if alts[i].1 < params.min_altitude_deg && alts[i + 1].1 >= params.min_altitude_deg {
            let aos_coarse = alts[i].0;
            // Find LOS: next time we drop below min.
            let mut j = i + 1;
            while j + 1 < alts.len() && alts[j].1 >= params.min_altitude_deg {
                j += 1;
            }
            let los_coarse = alts[j].0;
            if let Some(pass) =
                refine_pass(prop, observer, aos_coarse, los_coarse, params, view_windows)?
            {
                passes.push(pass);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    Ok(passes)
}

fn sample_visible(
    prop: &Propagator,
    observer: &Observer,
    utc: DateTime<Utc>,
    min_alt: f64,
    max_sun_alt: f64,
    require_illum: bool,
) -> Result<(bool, f64, f64, [f64; 3]), String> {
    let topo = prop.observe(observer, utc)?;
    let ecef = sat_ecef(prop, utc)?;
    let alt_ok = topo.altitude_deg >= min_alt;
    let illum_ok = if require_illum {
        pass_illumination_ok(observer, ecef, utc, max_sun_alt)
    } else {
        true
    };
    Ok((
        alt_ok && illum_ok,
        topo.altitude_deg,
        topo.azimuth_deg,
        ecef,
    ))
}

fn refine_pass(
    prop: &Propagator,
    observer: &Observer,
    aos_coarse: DateTime<Utc>,
    los_coarse: DateTime<Utc>,
    params: &PassSearchParams,
    view_windows: &[ViewWindow],
) -> Result<Option<VisiblePass>, String> {
    let geom_aos = refine_crossing(
        prop,
        observer,
        aos_coarse,
        aos_coarse + params.coarse_step,
        params.min_altitude_deg,
        params.fine_step,
        true,
    )?;
    let geom_los = refine_crossing(
        prop,
        observer,
        los_coarse - params.coarse_step,
        los_coarse,
        params.min_altitude_deg,
        params.fine_step,
        false,
    )?;
    if geom_los <= geom_aos {
        return Ok(None);
    }

    // Walk geometric pass; keep the longest contiguous "visible" segment.
    let mut best: Option<(DateTime<Utc>, DateTime<Utc>)> = None;
    let mut seg_start: Option<DateTime<Utc>> = None;
    let mut t = geom_aos;
    while t <= geom_los {
        let (ok, _, _, _) = sample_visible(
            prop,
            observer,
            t,
            params.min_altitude_deg,
            params.max_sun_alt_deg,
            params.require_illumination,
        )?;
        if ok {
            if seg_start.is_none() {
                seg_start = Some(t);
            }
        } else if let Some(s) = seg_start.take() {
            let end = t - params.fine_step;
            if end > s {
                let better = best.map(|(a, b)| (end - s) > (b - a)).unwrap_or(true);
                if better {
                    best = Some((s, end));
                }
            }
        }
        t += params.fine_step;
    }
    if let Some(s) = seg_start {
        let better = best.map(|(a, b)| (geom_los - s) > (b - a)).unwrap_or(true);
        if better {
            best = Some((s, geom_los));
        }
    }

    let Some((aos, los)) = best else {
        return Ok(None);
    };
    let duration_secs = (los - aos).num_seconds();
    if params.require_illumination && duration_secs < MIN_VISIBLE_DURATION_SECS {
        return Ok(None);
    }
    if los <= aos {
        return Ok(None);
    }

    // Peak inside visible window.
    let mut peak = aos;
    let mut max_alt = f64::NEG_INFINITY;
    let mut peak_az = 0.0;
    let mut peak_ecef = [0.0; 3];
    let mut peak_range = 0.0;
    let mut t = aos;
    while t <= los {
        let topo = prop.observe(observer, t)?;
        if topo.altitude_deg > max_alt {
            max_alt = topo.altitude_deg;
            peak = t;
            peak_az = topo.azimuth_deg;
            peak_range = topo.range_km;
            peak_ecef = sat_ecef(prop, t)?;
        }
        t += params.fine_step;
    }

    let phase = phase_angle_deg(peak_ecef, observer, peak);
    let peak_magnitude = apparent_magnitude(peak_range, phase, max_alt);
    let illuminated = params.require_illumination
        || pass_illumination_ok(observer, peak_ecef, peak, params.max_sun_alt_deg);

    // Sample track inside visible window.
    let mut track = Vec::new();
    let mut t = aos;
    while t <= los {
        let topo = prop.observe(observer, t)?;
        track.push(TrackSample::from_topo(t, topo));
        t += params.sample_step;
    }
    if track.last().map(|s| s.utc) != Some(los) {
        let topo = prop.observe(observer, los)?;
        track.push(TrackSample::from_topo(los, topo));
    }

    if !view_windows.is_empty() {
        let in_view = track.iter().any(|s| {
            view_windows
                .iter()
                .any(|vw| vw.contains(s.azimuth_deg, s.altitude_deg))
        });
        if !in_view {
            return Ok(None);
        }
    }

    Ok(Some(VisiblePass {
        aos,
        los,
        peak,
        max_altitude_deg: max_alt,
        peak_azimuth_deg: peak_az,
        illuminated,
        duration_secs,
        phase_angle_deg: phase,
        peak_magnitude,
        peak_range_km: peak_range,
        track,
    }))
}

/// Binary-ish refine of altitude crossing.
fn refine_crossing(
    prop: &Propagator,
    observer: &Observer,
    mut lo: DateTime<Utc>,
    mut hi: DateTime<Utc>,
    min_alt: f64,
    step: Duration,
    rising: bool,
) -> Result<DateTime<Utc>, String> {
    if hi <= lo {
        return Ok(lo);
    }
    while hi - lo > step {
        let mid = lo + (hi - lo) / 2;
        let alt = prop.observe(observer, mid)?.altitude_deg;
        if rising {
            if alt < min_alt {
                lo = mid;
            } else {
                hi = mid;
            }
        } else if alt >= min_alt {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(if rising { hi } else { lo })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::satellites::fixtures;

    #[test]
    fn finds_horizon_passes_without_illumination_filter() {
        let prop =
            Propagator::from_tle(Some("ISS"), fixtures::ISS_TLE_L1, fixtures::ISS_TLE_L2).unwrap();
        let epoch = prop.epoch_utc();
        let start = epoch;
        let end = epoch + Duration::hours(24);
        let obs = Observer::new(48.8566, 2.3522);
        let mut params = PassSearchParams::default();
        params.require_illumination = false;
        params.min_altitude_deg = 10.0;
        let passes = find_passes(&prop, &obs, start, end, &params, &[]).unwrap();
        assert!(
            !passes.is_empty(),
            "expected at least one geometric pass over Paris in 24h"
        );
        for p in &passes {
            assert!(p.los > p.aos);
            assert!(p.max_altitude_deg >= 10.0);
            assert!(!p.track.is_empty());
            assert!(p.duration_secs > 0);
            assert!(p.peak_magnitude.is_finite());
            let peak_topo = prop.observe(&obs, p.peak).unwrap();
            assert!((peak_topo.altitude_deg - p.max_altitude_deg).abs() < 0.5);
        }
    }

    #[test]
    fn illuminated_pass_duration_not_absurd() {
        let prop =
            Propagator::from_tle(Some("ISS"), fixtures::ISS_TLE_L1, fixtures::ISS_TLE_L2).unwrap();
        let epoch = prop.epoch_utc();
        let obs = Observer::new(48.8566, 2.3522);
        let params = PassSearchParams::default();
        let passes = find_passes(
            &prop,
            &obs,
            epoch,
            epoch + Duration::hours(48),
            &params,
            &[],
        )
        .unwrap();
        for p in &passes {
            // Visible window should stay under ~4 minutes for typical dusk/dawn segments.
            assert!(
                p.duration_secs <= 240,
                "duration {}s too long for illuminated window",
                p.duration_secs
            );
            assert!(p.duration_secs >= MIN_VISIBLE_DURATION_SECS);
            assert!((0.0..=180.0).contains(&p.phase_angle_deg));
        }
    }

    #[test]
    fn view_window_filters_passes() {
        let prop =
            Propagator::from_tle(Some("ISS"), fixtures::ISS_TLE_L1, fixtures::ISS_TLE_L2).unwrap();
        let epoch = prop.epoch_utc();
        let obs = Observer::new(48.8566, 2.3522);
        let mut params = PassSearchParams::default();
        params.require_illumination = false;
        let all = find_passes(
            &prop,
            &obs,
            epoch,
            epoch + Duration::hours(24),
            &params,
            &[],
        )
        .unwrap();
        let tiny = vec![ViewWindow::new(0.0, 1.0, 85.0, 90.0)];
        let filtered = find_passes(
            &prop,
            &obs,
            epoch,
            epoch + Duration::hours(24),
            &params,
            &tiny,
        )
        .unwrap();
        assert!(filtered.len() <= all.len());
    }

    #[test]
    fn coarse_vs_fine_peak_within_30s() {
        let prop =
            Propagator::from_tle(Some("ISS"), fixtures::ISS_TLE_L1, fixtures::ISS_TLE_L2).unwrap();
        let epoch = prop.epoch_utc();
        let obs = Observer::new(48.8566, 2.3522);
        let mut coarse = PassSearchParams::default();
        coarse.require_illumination = false;
        coarse.coarse_step = Duration::seconds(60);
        coarse.fine_step = Duration::seconds(1);
        let passes = find_passes(
            &prop,
            &obs,
            epoch,
            epoch + Duration::hours(12),
            &coarse,
            &[],
        )
        .unwrap();
        if let Some(p) = passes.first() {
            let mut t = p.aos;
            let mut best_t = p.aos;
            let mut best_alt = f64::NEG_INFINITY;
            while t <= p.los {
                let a = prop.observe(&obs, t).unwrap().altitude_deg;
                if a > best_alt {
                    best_alt = a;
                    best_t = t;
                }
                t += Duration::seconds(5);
            }
            let dt = (best_t - p.peak).num_seconds().abs();
            assert!(dt <= 30, "peak time mismatch {dt}s");
        }
    }
}
