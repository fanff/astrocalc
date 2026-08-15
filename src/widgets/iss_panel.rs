//! Helpers for ISS opportunity list presentation.

use chrono::DateTime;
use chrono::Utc;

use crate::satellites::{DiskTransit, VisiblePass, bearing_to_compass};

/// One row in the chronological ISS opportunity list.
pub enum IssOpportunity<'a> {
    Pass(&'a VisiblePass),
    Disk(&'a DiskTransit),
}

impl IssOpportunity<'_> {
    pub fn sort_time(&self) -> DateTime<Utc> {
        match self {
            IssOpportunity::Pass(p) => p.peak,
            IssOpportunity::Disk(e) => e.center_time,
        }
    }
}

/// Merge visible passes and disk events, sorted by event time ascending.
pub fn opportunity_list<'a>(
    passes: &'a [VisiblePass],
    sun_transits: &'a [DiskTransit],
    moon_transits: &'a [DiskTransit],
) -> Vec<IssOpportunity<'a>> {
    let mut rows: Vec<IssOpportunity<'a>> = passes
        .iter()
        .map(IssOpportunity::Pass)
        .chain(sun_transits.iter().map(IssOpportunity::Disk))
        .chain(moon_transits.iter().map(IssOpportunity::Disk))
        .collect();
    rows.sort_by_key(|o| o.sort_time());
    rows
}

/// Human description of how high the ISS gets during a pass.
pub fn pass_elevation_phrase(max_alt_deg: f64) -> &'static str {
    match max_alt_deg {
        a if a < 15.0 => "near the horizon",
        a if a < 30.0 => "low in the sky",
        a if a < 50.0 => "mid-sky",
        a if a < 70.0 => "high overhead",
        a if a < 85.0 => "nearly overhead",
        _ => "right overhead",
    }
}

/// Visible-pass duration in whole seconds (prefer illuminated visible window).
pub fn pass_duration_secs(pass: &VisiblePass) -> i64 {
    if pass.duration_secs > 0 {
        pass.duration_secs
    } else {
        (pass.los - pass.aos).num_seconds().max(0)
    }
}

/// Near-miss travel hint, e.g. `move ~12 km north-east`.
pub fn disk_move_hint(event: &DiskTransit) -> Option<String> {
    let km = event.move_hint_km.or(event.centerline_miss_km)?;
    if km < 0.05 {
        return Some("on the center-line".into());
    }
    let dir = event
        .move_hint_bearing_deg
        .map(bearing_to_compass)
        .unwrap_or("toward the path");
    let km_txt = if km < 10.0 {
        format!("{km:.1}")
    } else {
        format!("{:.0}", km.round())
    };
    Some(format!("move ~{km_txt} km {dir}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_phrases_cover_range() {
        assert_eq!(pass_elevation_phrase(8.0), "near the horizon");
        assert_eq!(pass_elevation_phrase(88.0), "right overhead");
        assert_eq!(pass_elevation_phrase(40.0), "mid-sky");
    }

    #[test]
    fn compass_octants() {
        assert_eq!(bearing_to_compass(0.0), "north");
        assert_eq!(bearing_to_compass(45.0), "north-east");
        assert_eq!(bearing_to_compass(90.0), "east");
        assert_eq!(bearing_to_compass(225.0), "south-west");
    }
}
