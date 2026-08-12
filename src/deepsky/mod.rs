use astro::{
    angle,
    coords::hr_angl_frm_observer_long,
    time,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use julian_day_converter::unixtime_to_julian_day;
use std::collections::{HashMap, HashSet};

use crate::deepsky::data::{CATALOG, DeepObject};
use crate::solarsystemcalc::{NightInfo, ObjectPosition, build_night_intervals};

pub mod data;

/// Meta for Gantt labels (type/description) keyed by display id.
pub type ObjectTypeMap = HashMap<String, String>;

/// Sample deep-sky alt/az over a night for selected catalog ids.
///
/// `selected_ids` are display ids (`M31`, `NGC7000`, …). Empty set → no positions.
/// Objects must parse RA/Dec and have `v_mag` when a mag limit is applied upstream.
pub fn calculate_deep_sky_positions(
    night: &NightInfo,
    lat_deg: f64,
    lon_deg: f64,
    freq_minutes: i64,
    selected_ids: &[String],
) -> (Vec<ObjectPosition>, ObjectTypeMap) {
    if selected_ids.is_empty() {
        return (Vec::new(), HashMap::new());
    }

    let want: HashSet<String> = selected_ids
        .iter()
        .map(|s| s.trim().to_uppercase().replace(' ', ""))
        .collect();

    let targets: Vec<&DeepObject> = CATALOG
        .objects
        .iter()
        .filter(|o| {
            o.display_id()
                .map(|id| want.contains(&id.to_uppercase().replace(' ', "")))
                .unwrap_or(false)
        })
        .collect();

    if targets.is_empty() {
        return (Vec::new(), HashMap::new());
    }

    let mut type_map = HashMap::new();
    for obj in &targets {
        if let Some(id) = obj.display_id() {
            type_map.insert(id, obj.type_label());
        }
    }

    let intervals = build_night_intervals(std::slice::from_ref(night), freq_minutes);
    let mut positions = Vec::new();

    for (date, ticks) in intervals {
        for datetime in ticks {
            for obj in &targets {
                if let Some(pos) = deep_sky_position_at(obj, date, datetime, lat_deg, lon_deg) {
                    positions.push(pos);
                }
            }
        }
    }

    (positions, type_map)
}

fn deep_sky_position_at(
    obj: &DeepObject,
    date: NaiveDate,
    datetime: DateTime<Utc>,
    lat_deg: f64,
    lon_deg: f64,
) -> Option<ObjectPosition> {
    let (ra, dec) = obj.ra_dec_rad()?;
    let name = obj.display_id()?;
    let mag = obj.v_mag.map(|m| m as f64).unwrap_or(99.0);

    let jd = unixtime_to_julian_day(datetime.timestamp());
    let gmst = time::mn_sidr(jd);
    let lat_rad = lat_deg.to_radians();
    let lon_rad = lon_deg.to_radians();

    let ra_norm = angle::limit_to_two_PI(ra);
    let hour_angle = hr_angl_frm_observer_long(gmst, -lon_rad, ra);
    let (az, alt) = astro::loc_hz_frm_eq!(hour_angle, dec, lat_rad);

    Some(ObjectPosition {
        name,
        utc_datetime: datetime,
        date,
        ra: ra_norm.to_degrees(),
        dec: dec.to_degrees(),
        altitude: alt.to_degrees(),
        azimuth: angle::limit_to_360(az.to_degrees() + 180.0),
        magnitude: mag,
        distance: 0.0,
        phase_ratio: 0.0,
    })
}

/// Convenience when only a single instant is needed (tests / debug).
pub fn deep_sky_positions_at(
    date: NaiveDate,
    datetime: DateTime<Utc>,
    lat_deg: f64,
    lon_deg: f64,
    mag_limit: f64,
) -> Vec<ObjectPosition> {
    let catalog = CATALOG.filter_magnitude(mag_limit as f32);
    catalog
        .objects
        .iter()
        .filter_map(|obj| deep_sky_position_at(obj, date, datetime, lat_deg, lon_deg))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepsky::data::{parse_dms_to_degrees, parse_hms_to_degrees};

    #[test]
    fn parse_m31_coords() {
        // M31 row in catalog: 00:42:44.33 / +41:16:07.5
        let ra = parse_hms_to_degrees("00:42:44.33").unwrap();
        let dec = parse_dms_to_degrees("+41:16:07.5").unwrap();
        assert!((ra - 10.6847).abs() < 0.01);
        assert!((dec - 41.26875).abs() < 0.01);
    }

    #[test]
    fn sample_selected_messier() {
        let night = NightInfo {
            date: NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(),
            night_start_ms: DateTime::<Utc>::from_timestamp(1_723_420_800, 0).unwrap(),
            night_end_ms: DateTime::<Utc>::from_timestamp(1_723_420_800, 0).unwrap()
                + Duration::hours(1),
        };
        let (pos, meta) = calculate_deep_sky_positions(
            &night,
            48.85,
            2.35,
            30,
            &["M31".into()],
        );
        assert!(!pos.is_empty());
        assert!(meta.contains_key("M31"));
        assert!(pos.iter().all(|p| p.name == "M31"));
    }
}
