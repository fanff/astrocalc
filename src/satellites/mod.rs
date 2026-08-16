//! ISS / satellite pass and transit predictions.
//!
//! Domain math lives here; TLE HTTP/file cache is infra-adjacent but kept with the
//! owning feature so panels can call a single facade.

mod brightness;
mod events;
mod fixtures;
mod illumination;
mod passes;
mod propagate;
mod tle;
mod transit;

pub use brightness::{cloud_label, magnitude_phrase, naked_eye_label};
pub use events::{DiskBody, DiskTransit, IssEventKind, TrackSample, VisiblePass};
pub use illumination::{
    DEFAULT_OBSERVER_SUN_MAX_ALT_DEG, moon_illum_pct, moon_phase_label, observer_sky_dark,
    observer_sun_altitude_deg, pass_illumination_ok, satellite_sunlit, sun_semi_diameter_deg,
};
pub use passes::{PassSearchParams, find_passes};
pub use propagate::{Observer, Propagator, Topocentric};
pub use tle::{
    CELESTRAK_ISS_TLE_URL, CachedTle, DEFAULT_TLE_FRESHNESS, ISS_NORAD_ID, PASS_STALE_AGE,
    TRANSIT_WARN_AGE, TleCache, fetch_iss_tle, parse_tle_text,
};
pub use transit::{TransitSearchParams, bearing_to_compass, find_disk_events};

use chrono::{DateTime, Duration, NaiveDate, Utc};

use crate::config::ViewWindow;

/// Horizon for ISS opportunity scan (calendar days from start date).
pub const ISS_PREDICT_DAY_COUNT: i64 = 60;

#[derive(Clone, Debug)]
pub struct IssPredictionBundle {
    pub tle: CachedTle,
    pub passes: Vec<VisiblePass>,
    pub sun_transits: Vec<DiskTransit>,
    pub moon_transits: Vec<DiskTransit>,
    pub computed_at: DateTime<Utc>,
}

/// Predict visible passes + disk events for `[day, day + day_count)` calendar days
/// (UTC midnights spanning that range).
pub fn predict_iss_window(
    tle: &CachedTle,
    observer: Observer,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    view_windows: &[ViewWindow],
    include_transits: bool,
) -> Result<IssPredictionBundle, String> {
    let prop = tle.propagator()?;
    let pass_params = PassSearchParams::default();
    let passes = find_passes(&prop, &observer, start, end, &pass_params, view_windows)?;

    let mut sun_transits = Vec::new();
    let mut moon_transits = Vec::new();
    if include_transits {
        let tp = TransitSearchParams::default();
        sun_transits = find_disk_events(
            &prop,
            &observer,
            DiskBody::Sun,
            start,
            end,
            &tp,
            view_windows,
        )?;
        moon_transits = find_disk_events(
            &prop,
            &observer,
            DiskBody::Moon,
            start,
            end,
            &tp,
            view_windows,
        )?;
    }

    Ok(IssPredictionBundle {
        tle: tle.clone(),
        passes,
        sun_transits,
        moon_transits,
        computed_at: Utc::now(),
    })
}

/// UTC window covering `date` .. `date + days` (inclusive start, exclusive end+1 day).
pub fn utc_window_for_dates(date: NaiveDate, days: i64) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = date.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = (date + Duration::days(days))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    (start, end)
}

/// Fetch/cache TLE then predict (blocking; for Bind).
pub fn fetch_and_predict(
    cache: &TleCache,
    force_tle: bool,
    lat_deg: f64,
    lon_deg: f64,
    date: NaiveDate,
    day_count: i64,
    view_windows: &[ViewWindow],
    include_transits: bool,
) -> Result<IssPredictionBundle, String> {
    let tle = cache.get_or_fetch(force_tle)?;
    let (start, end) = utc_window_for_dates(date, day_count);
    predict_iss_window(
        &tle,
        Observer::new(lat_deg, lon_deg),
        start,
        end,
        view_windows,
        include_transits,
    )
}
