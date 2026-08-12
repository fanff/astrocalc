//! Observer timezone from map coordinates + UTC/local formatting helpers.

use chrono::{DateTime, Timelike, Utc};
use chrono_tz::Tz;
use once_cell::sync::Lazy;
use std::str::FromStr;
use tzf_rs::DefaultFinder;

static TZ_FINDER: Lazy<DefaultFinder> = Lazy::new(DefaultFinder::new);

/// Resolve IANA timezone for a site from latitude/longitude (DST handled by `chrono-tz`).
pub fn site_tz_from_lat_lon(lat: f64, lon: f64) -> Tz {
    let name = TZ_FINDER.get_tz_name(lon, lat);
    Tz::from_str(name).unwrap_or(Tz::UTC)
}

pub fn format_hm_utc(dt: DateTime<Utc>) -> String {
    format!("{:02}:{:02}", dt.hour(), dt.minute())
}

pub fn format_hm_local(dt: DateTime<Utc>, tz: Tz) -> String {
    let local = dt.with_timezone(&tz);
    format!("{:02}:{:02}", local.hour(), local.minute())
}

/// Axis tick: `HH:MM UTC`
pub fn format_axis_utc(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|dt| format!("{} UTC", format_hm_utc(dt)))
        .unwrap_or_default()
}

/// Axis tick: `HH:MM Europe/Paris` (zone id)
pub fn format_axis_local(ms: i64, tz: Tz) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|dt| format!("{} {}", format_hm_local(dt, tz), tz.name()))
        .unwrap_or_default()
}

/// Tooltip-friendly UTC + local block.
pub fn format_utc_local_block(dt: DateTime<Utc>, tz: Tz) -> String {
    let local = dt.with_timezone(&tz);
    format!(
        "UTC: {}\n{}: {}",
        dt.format("%Y-%m-%d %H:%M"),
        tz.name(),
        local.format("%Y-%m-%d %H:%M")
    )
}

/// Short tooltip lines: `HH:MM UTC` / `HH:MM Zone`
pub fn format_utc_local_hm(dt: DateTime<Utc>, tz: Tz) -> String {
    format!(
        "{} UTC\n{} {}",
        format_hm_utc(dt),
        format_hm_local(dt, tz),
        tz.name()
    )
}
