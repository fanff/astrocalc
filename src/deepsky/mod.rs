use astro::{angle, coords::hr_angl_frm_observer_long, time};
use chrono::NaiveDate;
use julian_day_converter::unixtime_to_julian_day;
use std::collections::{HashMap, HashSet};

use crate::deepsky::data::{CATALOG, DeepObject};
use crate::models::{ObjectPositionInsert, ObjectPositionStored, POSITION_KIND_DSO};
use crate::panels::LatLon;
use crate::solarsystemcalc::{NightInfo, ObjectPosition, build_night_intervals};
use diesel::{Connection, SqliteConnection};

pub mod data;

/// Meta for Gantt labels (type/description) keyed by display id.
pub type ObjectTypeMap = HashMap<String, String>;

fn normalize_dso_id(id: &str) -> String {
    id.trim().to_uppercase().replace(' ', "")
}

/// Type labels for display ids from the embedded catalog.
pub fn dso_type_map_for_ids(ids: &[String]) -> ObjectTypeMap {
    let want: HashSet<String> = ids.iter().map(|s| normalize_dso_id(s)).collect();
    let mut map = HashMap::new();
    for obj in &CATALOG.objects {
        if let Some(id) = obj.display_id() {
            if want.contains(&normalize_dso_id(&id)) {
                map.insert(id, obj.type_label());
            }
        }
    }
    map
}

/// Read DSO blob for a night and return only selected ids (no computation).
pub fn cached_dso_positions(
    conn: &mut SqliteConnection,
    date: NaiveDate,
    lat_deg: f64,
    lon_deg: f64,
    selected_ids: &[String],
) -> Vec<ObjectPosition> {
    if selected_ids.is_empty() {
        return Vec::new();
    }
    let snapped = LatLon {
        lat: lat_deg,
        lon: lon_deg,
    }
    .snap(2);
    let want: HashSet<String> = selected_ids.iter().map(|s| normalize_dso_id(s)).collect();
    ObjectPositionStored::read_from_db_kind(conn, date, snapped, POSITION_KIND_DSO)
        .into_iter()
        .filter(|p| want.contains(&normalize_dso_id(&p.name)))
        .collect()
}

/// Calendar nights (among `dates`) where at least one selected DSO id is not yet cached.
pub fn nights_needing_selected_dso(
    conn: &mut SqliteConnection,
    lat_deg: f64,
    lon_deg: f64,
    dates: &[NaiveDate],
    selected_ids: &[String],
) -> Vec<NaiveDate> {
    if selected_ids.is_empty() {
        return Vec::new();
    }
    let snapped = LatLon {
        lat: lat_deg,
        lon: lon_deg,
    }
    .snap(2);
    let want: Vec<String> = selected_ids.iter().map(|s| normalize_dso_id(s)).collect();
    let mut out = Vec::new();
    for &date in dates {
        let existing =
            ObjectPositionStored::read_from_db_kind(conn, date, snapped, POSITION_KIND_DSO);
        let have: HashSet<String> = existing.iter().map(|p| normalize_dso_id(&p.name)).collect();
        if want.iter().any(|id| !have.contains(id)) {
            out.push(date);
        }
    }
    out
}

/// Ensure selected DSO tracks for each night in `dates` (used by background Bind batches).
pub fn ensure_dso_batch(
    database_url: &str,
    lat_deg: f64,
    lon_deg: f64,
    freq_minutes: i64,
    selected_ids: &[String],
    dates: &[NaiveDate],
) {
    if selected_ids.is_empty() || dates.is_empty() {
        return;
    }
    let mut conn = SqliteConnection::establish(database_url)
        .unwrap_or_else(|_| panic!("Error connecting to {database_url}"));
    let snapped = LatLon {
        lat: lat_deg,
        lon: lon_deg,
    }
    .snap(2);
    for &date in dates {
        let Some(date_info) = crate::models::DateInfo::from_db(&mut conn, date, &snapped) else {
            continue;
        };
        let night = date_info.as_nightinfo();
        let _ = ensure_dso_positions(
            &mut conn,
            &night,
            lat_deg,
            lon_deg,
            freq_minutes,
            selected_ids,
        );
    }
}

/// Ensure selected DSO tracks are cached for a night; compute only missing ids.
///
/// Returns positions for the requested ids (from cache + any newly computed), plus type labels.
pub fn ensure_dso_positions(
    conn: &mut SqliteConnection,
    night: &NightInfo,
    lat_deg: f64,
    lon_deg: f64,
    freq_minutes: i64,
    selected_ids: &[String],
) -> (Vec<ObjectPosition>, ObjectTypeMap) {
    let type_map = dso_type_map_for_ids(selected_ids);
    if selected_ids.is_empty() {
        return (Vec::new(), type_map);
    }

    let snapped = LatLon {
        lat: lat_deg,
        lon: lon_deg,
    }
    .snap(2);

    let existing =
        ObjectPositionStored::read_from_db_kind(conn, night.date, snapped, POSITION_KIND_DSO);

    let have: HashSet<String> = existing.iter().map(|p| normalize_dso_id(&p.name)).collect();

    let missing: Vec<String> = selected_ids
        .iter()
        .filter(|id| !have.contains(&normalize_dso_id(id)))
        .cloned()
        .collect();

    let mut merged = existing;
    if !missing.is_empty() {
        let (new_pos, _) =
            calculate_deep_sky_positions(night, lat_deg, lon_deg, freq_minutes, &missing);
        merged.extend(new_pos);
        ObjectPositionInsert::upsert_date(
            conn,
            night.date,
            snapped.lat,
            snapped.lon,
            POSITION_KIND_DSO,
            &merged,
        );
    }

    let want: HashSet<String> = selected_ids.iter().map(|s| normalize_dso_id(s)).collect();
    let filtered: Vec<ObjectPosition> = merged
        .into_iter()
        .filter(|p| want.contains(&normalize_dso_id(&p.name)))
        .collect();

    (filtered, type_map)
}

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

    // Precompute per-object constants (RA/Dec parse + display name once).
    struct TargetSample {
        name: String,
        ra: f64,
        dec: f64,
        mag: f64,
    }
    let samples: Vec<TargetSample> = targets
        .iter()
        .filter_map(|obj| {
            let (ra, dec) = obj.ra_dec_rad()?;
            let name = obj.display_id()?;
            let mag = obj.v_mag.map(|m| m as f64).unwrap_or(99.0);
            Some(TargetSample { name, ra, dec, mag })
        })
        .collect();

    let lat_rad = lat_deg.to_radians();
    let lon_rad = lon_deg.to_radians();

    for (date, ticks) in intervals {
        for datetime in ticks {
            let jd = unixtime_to_julian_day(datetime.timestamp());
            let gmst = time::mn_sidr(jd);
            for sample in &samples {
                let ra_norm = angle::limit_to_two_PI(sample.ra);
                let hour_angle = hr_angl_frm_observer_long(gmst, -lon_rad, sample.ra);
                let (az, alt) = astro::loc_hz_frm_eq!(hour_angle, sample.dec, lat_rad);
                positions.push(ObjectPosition {
                    name: sample.name.clone(),
                    utc_datetime: datetime,
                    date,
                    ra: ra_norm.to_degrees(),
                    dec: sample.dec.to_degrees(),
                    altitude: alt.to_degrees(),
                    azimuth: angle::limit_to_360(az.to_degrees() + 180.0),
                    magnitude: sample.mag,
                    distance: 0.0,
                    phase_ratio: 0.0,
                });
            }
        }
    }

    (positions, type_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MIGRATIONS;
    use crate::deepsky::data::{parse_dms_to_degrees, parse_hms_to_degrees};
    use chrono::{DateTime, Duration, Utc};
    use diesel::Connection;
    use diesel_migrations::MigrationHarness;

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
        let (pos, meta) = calculate_deep_sky_positions(&night, 48.85, 2.35, 30, &["M31".into()]);
        assert!(!pos.is_empty());
        assert!(meta.contains_key("M31"));
        assert!(pos.iter().all(|p| p.name == "M31"));
    }

    #[test]
    fn ensure_dso_merges_missing_ids_only() {
        let path = std::env::temp_dir().join("astrocalc_dso_ensure_test.db");
        let _ = std::fs::remove_file(&path);
        let url = path.to_str().unwrap().to_string();
        let mut conn = SqliteConnection::establish(&url).unwrap();
        conn.run_pending_migrations(MIGRATIONS).expect("migrations");

        let night = NightInfo {
            date: NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(),
            night_start_ms: DateTime::<Utc>::from_timestamp(1_723_420_800, 0).unwrap(),
            night_end_ms: DateTime::<Utc>::from_timestamp(1_723_420_800, 0).unwrap()
                + Duration::hours(2),
        };

        let (first, _) = ensure_dso_positions(&mut conn, &night, 48.85, 2.35, 30, &["M31".into()]);
        assert!(!first.is_empty());
        let first_len = first.len();

        let (second, _) = ensure_dso_positions(
            &mut conn,
            &night,
            48.85,
            2.35,
            30,
            &["M31".into(), "M42".into()],
        );
        assert!(second.iter().any(|p| p.name == "M31"));
        assert!(second.iter().any(|p| p.name == "M42"));
        // Cached M31 samples should still be present (same night length / freq).
        assert!(second.iter().filter(|p| p.name == "M31").count() == first_len);

        let _ = std::fs::remove_file(&path);
    }
}
