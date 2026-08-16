use crate::config::AppSettings;
use crate::panels::LatLon;
use crate::solarsystemcalc::{NightInfo, ObjectPosition};
use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use diesel::{BoolExpressionMethods, ExpressionMethods, QueryDsl, RunQueryDsl};

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = crate::schema::dateinfo)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct DateInfo {
    pub id: i32,
    pub date: String,
    pub lat_sector: f64,
    pub lon_sector: f64,
    pub night_start_ms: i64,
    pub night_end_ms: i64,
}

impl DateInfo {
    pub fn as_nightinfo(&self) -> NightInfo {
        let nd = NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").unwrap();
        NightInfo {
            date: nd,
            night_start_ms: DateTime::<Utc>::from_timestamp_millis(self.night_start_ms).unwrap(),
            night_end_ms: DateTime::<Utc>::from_timestamp_millis(self.night_end_ms).unwrap(),
        }
    }
    pub fn available_dates_from_db(
        conn: &mut SqliteConnection,
        lat_lon_snapped: &LatLon,
    ) -> Vec<NaiveDate> {
        use crate::schema::dateinfo::dsl::*;
        let results = dateinfo
            .filter(lat_sector.eq(lat_lon_snapped.lat))
            .filter(lon_sector.eq(lat_lon_snapped.lon))
            .select(date)
            .distinct()
            .load::<String>(conn)
            .expect("Error loading dates");
        let mut dates: Vec<NaiveDate> = Vec::new();
        for d in results {
            let nd = NaiveDate::parse_from_str(&d, "%Y-%m-%d").unwrap();
            dates.push(nd);
        }
        dates
    }
    pub fn from_db(
        conn: &mut SqliteConnection,
        date_at: NaiveDate,
        lat_lon_snapped: &LatLon,
    ) -> Option<DateInfo> {
        use crate::schema::dateinfo::dsl::*;
        let target_date: String = date_at.to_string();
        let results = dateinfo
            .filter(date.eq(target_date))
            .filter(lat_sector.eq(lat_lon_snapped.lat))
            .filter(lon_sector.eq(lat_lon_snapped.lon))
            .select(DateInfo::as_select())
            .load::<DateInfo>(conn)
            .expect("Error loading date info");
        if results.len() > 0 {
            Some(results[0].clone())
        } else {
            None
        }
    }
    pub fn from_db_range(
        conn: &mut SqliteConnection,
        date_start: NaiveDate,
        date_end: NaiveDate,
        lat_lon_snapped: &LatLon,
    ) -> Vec<NightInfo> {
        use crate::schema::dateinfo::dsl::*;
        let target_date_start: String = date_start.to_string();
        let target_date_end: String = date_end.to_string();
        let results = dateinfo
            .filter(date.ge(target_date_start))
            .filter(date.le(target_date_end))
            .filter(lat_sector.eq(lat_lon_snapped.lat))
            .filter(lon_sector.eq(lat_lon_snapped.lon))
            .select(DateInfo::as_select())
            .load::<DateInfo>(conn)
            .expect("Error loading date info")
            .iter()
            .map(|f| f.as_nightinfo())
            .collect();
        results
    }
    pub fn get_night_start_end(&self) -> (i64, i64) {
        (self.night_start_ms, self.night_end_ms)
    }
}
#[derive(Insertable)]
#[diesel(table_name = crate::schema::dateinfo)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct DateInfoInsert {
    pub date: String,
    pub lat_sector: f64,
    pub lon_sector: f64,
    pub night_start_ms: i64,
    pub night_end_ms: i64,
}
impl DateInfoInsert {
    pub fn from_night_info(
        night_info: &NightInfo,
        lat_sector_val: f64,
        lon_sector_val: f64,
    ) -> Self {
        Self {
            date: night_info.date.to_string(),
            lat_sector: lat_sector_val,
            lon_sector: lon_sector_val,
            night_start_ms: night_info.night_start_ms.timestamp_millis(),
            night_end_ms: night_info.night_end_ms.timestamp_millis(),
        }
    }
    pub fn from_vec(
        night_info_vec: &Vec<&NightInfo>,
        lat_sector_val: f64,
        lon_sector_val: f64,
    ) -> Vec<Self> {
        let mut inserts: Vec<Self> = Vec::new();
        for ni in night_info_vec {
            inserts.push(Self::from_night_info(ni, lat_sector_val, lon_sector_val));
        }
        inserts
    }
    pub fn insert_from_vec(
        conn: &mut SqliteConnection,
        night_info_vec: &Vec<&NightInfo>,
        lat_sector_val: f64,
        lon_sector_val: f64,
    ) {
        use crate::schema::dateinfo;
        let new_elements = DateInfoInsert::from_vec(night_info_vec, lat_sector_val, lon_sector_val);
        let q = diesel::insert_into(dateinfo::table)
            .values(new_elements)
            //.returning(DateInfo::as_returning())
            .execute(conn)
            .expect("error saving");
    }
}
/// Solar-system planet/Moon blob family.
pub const POSITION_KIND_SOLAR: &str = "solar";
/// Deep-sky catalog blob family (selected ids merged over time).
pub const POSITION_KIND_DSO: &str = "dso";

#[derive(Queryable, Selectable, QueryableByName)]
#[diesel(table_name = crate::schema::objectposition)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ObjectPositionStored {
    pub id: i32,
    pub date: String,
    pub lat_sector: f64,
    pub lon_sector: f64,
    pub data_chunk: Vec<u8>,
    pub calculated_at_ms: i64,
    pub kind: String,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::objectposition)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ObjectPositionInsert {
    pub date: String,
    pub lat_sector: f64,
    pub lon_sector: f64,
    pub data_chunk: Vec<u8>,
    pub calculated_at_ms: i64,
    pub kind: String,
}

impl ObjectPositionStored {
    pub fn available_days(conn: &mut SqliteConnection, lat_lon_snapped: &LatLon) -> Vec<NaiveDate> {
        Self::available_days_kind(conn, lat_lon_snapped, POSITION_KIND_SOLAR)
    }

    pub fn available_days_kind(
        conn: &mut SqliteConnection,
        lat_lon_snapped: &LatLon,
        kind_val: &str,
    ) -> Vec<NaiveDate> {
        use crate::schema::objectposition::dsl::*;
        let results = objectposition
            .filter(lat_sector.eq(lat_lon_snapped.lat))
            .filter(lon_sector.eq(lat_lon_snapped.lon))
            .filter(kind.eq(kind_val))
            .select(date)
            .distinct()
            .load::<String>(conn)
            .expect("Error loading dates");
        let mut dates: Vec<NaiveDate> = Vec::new();
        for d in results {
            let nd = NaiveDate::parse_from_str(&d, "%Y-%m-%d").unwrap();
            dates.push(nd);
        }
        dates
    }

    pub fn read_from_db(
        conn: &mut SqliteConnection,
        date_at: NaiveDate,
        lat_lon_snapped: LatLon,
    ) -> Vec<ObjectPosition> {
        Self::read_from_db_kind(conn, date_at, lat_lon_snapped, POSITION_KIND_SOLAR)
    }

    pub fn read_from_db_kind(
        conn: &mut SqliteConnection,
        date_at: NaiveDate,
        lat_lon_snapped: LatLon,
        kind_val: &str,
    ) -> Vec<ObjectPosition> {
        use crate::schema::objectposition::dsl::*;
        let target_date: String = date_at.to_string();
        let results = objectposition
            .filter(date.eq(target_date))
            .filter(lat_sector.eq(lat_lon_snapped.lat))
            .filter(lon_sector.eq(lat_lon_snapped.lon))
            .filter(kind.eq(kind_val))
            .select(ObjectPositionStored::as_select())
            .load::<ObjectPositionStored>(conn)
            .expect("Error loading object positions");
        if results.len() > 0 {
            let stored = &results[0].data_chunk;
            let (decoded, _len): (Vec<ObjectPosition>, usize) =
                bincode::decode_from_slice(stored, bincode::config::standard()).unwrap();
            decoded
        } else {
            Vec::new()
        }
    }
}

impl ObjectPositionInsert {
    pub fn insert_date(
        conn: &mut SqliteConnection,
        date: NaiveDate,
        lat_sector: f64,
        lon_sector: f64,
        op_vec: &[ObjectPosition],
    ) {
        Self::upsert_date(
            conn,
            date,
            lat_sector,
            lon_sector,
            POSITION_KIND_SOLAR,
            op_vec,
        );
    }

    pub fn upsert_date(
        conn: &mut SqliteConnection,
        date_at: NaiveDate,
        lat_sector_val: f64,
        lon_sector_val: f64,
        kind_val: &str,
        op_vec: &[ObjectPosition],
    ) {
        use crate::schema::objectposition::dsl::*;
        let now_utc = chrono::Utc::now().timestamp_millis();
        let encoded: Vec<u8> = bincode::encode_to_vec(op_vec, bincode::config::standard()).unwrap();
        let new_element = ObjectPositionInsert {
            calculated_at_ms: now_utc,
            date: date_at.to_string(),
            lat_sector: lat_sector_val,
            lon_sector: lon_sector_val,
            data_chunk: encoded,
            kind: kind_val.to_string(),
        };

        // SQLite unique index: (date, lat_sector, lon_sector, kind)
        diesel::delete(
            objectposition.filter(
                date.eq(date_at.to_string())
                    .and(lat_sector.eq(lat_sector_val))
                    .and(lon_sector.eq(lon_sector_val))
                    .and(kind.eq(kind_val)),
            ),
        )
        .execute(conn)
        .expect("error deleting prior objectposition row");

        diesel::insert_into(objectposition)
            .values(new_element)
            .execute(conn)
            .expect("error saving objectposition");
    }
}

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = crate::schema::app_settings)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct AppSettingsRow {
    pub id: i32,
    pub lat: f64,
    pub lon: f64,
    pub view_windows_json: String,
    pub bortle_class: i32,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::app_settings)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct AppSettingsUpsert {
    pub id: i32,
    pub lat: f64,
    pub lon: f64,
    pub view_windows_json: String,
    pub bortle_class: i32,
}

impl AppSettingsRow {
    pub fn into_settings(self) -> Result<AppSettings, String> {
        AppSettings::from_parts(
            self.lat,
            self.lon,
            &self.view_windows_json,
            self.bortle_class.clamp(1, 9) as u8,
        )
    }

    /// Load settings from DB, or seed Paris defaults when the table is empty.
    pub fn load_or_seed(conn: &mut SqliteConnection) -> Result<AppSettings, String> {
        use crate::schema::app_settings::dsl::*;
        let rows = app_settings
            .filter(id.eq(1))
            .select(AppSettingsRow::as_select())
            .load::<AppSettingsRow>(conn)
            .map_err(|e| format!("load app_settings: {e}"))?;
        if let Some(row) = rows.into_iter().next() {
            return row.into_settings();
        }
        let defaults = AppSettings::paris_defaults();
        Self::upsert(conn, &defaults)?;
        Ok(defaults)
    }

    pub fn upsert(conn: &mut SqliteConnection, settings: &AppSettings) -> Result<(), String> {
        use crate::schema::app_settings;
        let json = settings
            .view_windows_json()
            .map_err(|e| format!("encode view_windows: {e}"))?;
        let row = AppSettingsUpsert {
            id: 1,
            lat: settings.lat,
            lon: settings.lon,
            view_windows_json: json,
            bortle_class: settings.bortle_class as i32,
        };
        diesel::insert_into(app_settings::table)
            .values(&row)
            .on_conflict(app_settings::id)
            .do_update()
            .set(&row)
            .execute(conn)
            .map_err(|e| format!("upsert app_settings: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod app_settings_tests {
    use super::*;
    use diesel::Connection;

    fn setup_conn() -> SqliteConnection {
        let mut conn = SqliteConnection::establish(":memory:").unwrap();
        diesel::sql_query(
            "CREATE TABLE app_settings (
                id INTEGER NOT NULL PRIMARY KEY CHECK (id = 1),
                lat DOUBLE NOT NULL,
                lon DOUBLE NOT NULL,
                view_windows_json TEXT NOT NULL,
                bortle_class INTEGER NOT NULL DEFAULT 5
            )",
        )
        .execute(&mut conn)
        .unwrap();
        conn
    }

    #[test]
    fn load_or_seed_inserts_paris_defaults() {
        let mut conn = setup_conn();
        let settings = AppSettingsRow::load_or_seed(&mut conn).unwrap();
        assert!(settings.is_valid());
        assert!((settings.lat - 48.8566).abs() < 1e-9);
        let again = AppSettingsRow::load_or_seed(&mut conn).unwrap();
        assert_eq!(settings, again);
    }

    #[test]
    fn upsert_round_trip() {
        let mut conn = setup_conn();
        let mut settings = AppSettings::paris_defaults();
        settings.lat = 45.0;
        settings.lon = 1.0;
        AppSettingsRow::upsert(&mut conn, &settings).unwrap();
        let loaded = AppSettingsRow::load_or_seed(&mut conn).unwrap();
        assert!((loaded.lat - 45.0).abs() < 1e-9);
        assert!((loaded.lon - 1.0).abs() < 1e-9);
        assert_eq!(loaded.view_windows, settings.view_windows);
    }
}

/// Event kinds stored in `iss_events.kind`.
pub const ISS_KIND_VISIBLE_PASS: &str = "visible_pass";
pub const ISS_KIND_SUN_TRANSIT: &str = "sun_transit";
pub const ISS_KIND_MOON_TRANSIT: &str = "moon_transit";

#[derive(Queryable, Selectable, Clone, Debug)]
#[diesel(table_name = crate::schema::iss_events)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct IssEventRow {
    pub id: i32,
    pub kind: String,
    pub lat: f64,
    pub lon: f64,
    pub tle_epoch_ms: i64,
    pub computed_at_ms: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub peak_ms: i64,
    pub payload_json: String,
}

#[derive(Insertable)]
#[diesel(table_name = crate::schema::iss_events)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct IssEventInsert {
    pub kind: String,
    pub lat: f64,
    pub lon: f64,
    pub tle_epoch_ms: i64,
    pub computed_at_ms: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub peak_ms: i64,
    pub payload_json: String,
}

impl IssEventInsert {
    pub fn from_bundle(
        bundle: &crate::satellites::IssPredictionBundle,
        lat: f64,
        lon: f64,
    ) -> Result<Vec<Self>, String> {
        let tle_epoch_ms = bundle.tle.tle_epoch.timestamp_millis();
        let computed_at_ms = bundle.computed_at.timestamp_millis();
        let mut rows = Vec::new();
        for p in &bundle.passes {
            rows.push(Self {
                kind: ISS_KIND_VISIBLE_PASS.to_string(),
                lat,
                lon,
                tle_epoch_ms,
                computed_at_ms,
                start_ms: p.aos.timestamp_millis(),
                end_ms: p.los.timestamp_millis(),
                peak_ms: p.peak.timestamp_millis(),
                payload_json: serde_json::to_string(p)
                    .map_err(|e| format!("serialize pass: {e}"))?,
            });
        }
        for e in &bundle.sun_transits {
            rows.push(Self {
                kind: ISS_KIND_SUN_TRANSIT.to_string(),
                lat,
                lon,
                tle_epoch_ms,
                computed_at_ms,
                start_ms: e.center_time.timestamp_millis(),
                end_ms: e.center_time.timestamp_millis(),
                peak_ms: e.center_time.timestamp_millis(),
                payload_json: serde_json::to_string(e)
                    .map_err(|e| format!("serialize sun transit: {e}"))?,
            });
        }
        for e in &bundle.moon_transits {
            rows.push(Self {
                kind: ISS_KIND_MOON_TRANSIT.to_string(),
                lat,
                lon,
                tle_epoch_ms,
                computed_at_ms,
                start_ms: e.center_time.timestamp_millis(),
                end_ms: e.center_time.timestamp_millis(),
                peak_ms: e.center_time.timestamp_millis(),
                payload_json: serde_json::to_string(e)
                    .map_err(|e| format!("serialize moon transit: {e}"))?,
            });
        }
        Ok(rows)
    }
}

impl IssEventRow {
    /// Replace all ISS events for a site (full-precision lat/lon).
    pub fn replace_for_site(
        conn: &mut SqliteConnection,
        lat: f64,
        lon: f64,
        rows: &[IssEventInsert],
    ) -> Result<(), String> {
        use crate::schema::iss_events::dsl;
        diesel::delete(
            dsl::iss_events
                .filter(dsl::lat.eq(lat))
                .filter(dsl::lon.eq(lon)),
        )
        .execute(conn)
        .map_err(|e| format!("delete iss_events: {e}"))?;
        if !rows.is_empty() {
            diesel::insert_into(crate::schema::iss_events::table)
                .values(rows)
                .execute(conn)
                .map_err(|e| format!("insert iss_events: {e}"))?;
        }
        Ok(())
    }

    pub fn load_for_site_range(
        conn: &mut SqliteConnection,
        lat: f64,
        lon: f64,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<Self>, String> {
        use crate::schema::iss_events::dsl;
        dsl::iss_events
            .filter(dsl::lat.eq(lat))
            .filter(dsl::lon.eq(lon))
            .filter(dsl::peak_ms.ge(start_ms))
            .filter(dsl::peak_ms.lt(end_ms))
            .order(dsl::peak_ms.asc())
            .select(Self::as_select())
            .load::<Self>(conn)
            .map_err(|e| format!("load iss_events: {e}"))
    }

    /// Rebuild a prediction bundle from cached rows (no SGP4). Returns `None` if empty.
    /// TLE metadata prefers on-disk `TleCache`; otherwise uses row provenance for UI ages.
    pub fn try_load_bundle(
        conn: &mut SqliteConnection,
        lat: f64,
        lon: f64,
        start_ms: i64,
        end_ms: i64,
        tle_cache: &crate::satellites::TleCache,
    ) -> Result<Option<crate::satellites::IssPredictionBundle>, String> {
        let rows = Self::load_for_site_range(conn, lat, lon, start_ms, end_ms)?;
        if rows.is_empty() {
            return Ok(None);
        }

        let mut passes = Vec::new();
        let mut sun_transits = Vec::new();
        let mut moon_transits = Vec::new();
        let mut tle_epoch_ms = rows[0].tle_epoch_ms;
        let mut computed_at_ms = rows[0].computed_at_ms;

        for row in &rows {
            tle_epoch_ms = row.tle_epoch_ms;
            computed_at_ms = row.computed_at_ms;
            match row.kind.as_str() {
                ISS_KIND_VISIBLE_PASS => {
                    let p: crate::satellites::VisiblePass = serde_json::from_str(&row.payload_json)
                        .map_err(|e| format!("deserialize visible_pass: {e}"))?;
                    passes.push(p);
                }
                ISS_KIND_SUN_TRANSIT => {
                    let e: crate::satellites::DiskTransit = serde_json::from_str(&row.payload_json)
                        .map_err(|e| format!("deserialize sun_transit: {e}"))?;
                    sun_transits.push(e);
                }
                ISS_KIND_MOON_TRANSIT => {
                    let e: crate::satellites::DiskTransit = serde_json::from_str(&row.payload_json)
                        .map_err(|e| format!("deserialize moon_transit: {e}"))?;
                    moon_transits.push(e);
                }
                other => {
                    return Err(format!("unknown iss_events kind: {other}"));
                }
            }
        }

        let tle = tle_cache.load().unwrap_or_else(|| {
            let epoch =
                DateTime::<Utc>::from_timestamp_millis(tle_epoch_ms).unwrap_or_else(|| Utc::now());
            let computed = DateTime::<Utc>::from_timestamp_millis(computed_at_ms)
                .unwrap_or_else(|| Utc::now());
            crate::satellites::CachedTle {
                name: "ISS (ZARYA)".into(),
                line1: String::new(),
                line2: String::new(),
                fetched_at: computed,
                tle_epoch: epoch,
            }
        });

        let computed_at =
            DateTime::<Utc>::from_timestamp_millis(computed_at_ms).unwrap_or_else(|| Utc::now());

        Ok(Some(crate::satellites::IssPredictionBundle {
            tle,
            passes,
            sun_transits,
            moon_transits,
            computed_at,
        }))
    }
}
