use crate::config::AppSettings;
use crate::panels::LatLon;
use crate::solarsystemcalc::{NightInfo, ObjectPosition};
use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;

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
#[derive(Queryable, Selectable, QueryableByName)]
#[diesel(table_name = crate::schema::objectposition)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ObjectPositionStored {
    pub id: i32,
    pub date: String,
    pub lat_sector: f64,
    pub lon_sector: f64,
    pub data_chunk: Vec<u8>,

    ///
    pub calculated_at_ms: i64,
}

#[derive(Insertable)]
#[diesel(table_name = crate::schema::objectposition)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ObjectPositionInsert {
    pub date: String,
    pub lat_sector: f64,
    pub lon_sector: f64,
    pub data_chunk: Vec<u8>,

    ///
    pub calculated_at_ms: i64,
}

impl ObjectPositionStored {
    pub fn available_days(conn: &mut SqliteConnection, lat_lon_snapped: &LatLon) -> Vec<NaiveDate> {
        use crate::schema::objectposition::dsl::*;
        let results = objectposition
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
    pub fn read_from_db(
        conn: &mut SqliteConnection,
        date_at: NaiveDate,
        lat_lon_snapped: LatLon,
    ) -> Vec<ObjectPosition> {
        use crate::schema::objectposition::dsl::*;
        let target_date: String = date_at.to_string();
        let results = objectposition
            .filter(date.eq(target_date))
            .filter(lat_sector.eq(lat_lon_snapped.lat))
            .filter(lon_sector.eq(lat_lon_snapped.lon))
            .select(ObjectPositionStored::as_select())
            .load::<ObjectPositionStored>(conn)
            .expect("Error loading object positions");
        if results.len() > 0 {
            let stored = &results[0].data_chunk;
            let (decoded, len): (Vec<ObjectPosition>, usize) =
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
        op_vec: &Vec<ObjectPosition>,
    ) {
        println!(
            "Inserting {} object positions into the database.",
            op_vec.len()
        );
        use crate::schema::objectposition;
        let now_utc = chrono::Utc::now().timestamp_millis();
        let encoded: Vec<u8> =
            bincode::encode_to_vec(&op_vec, bincode::config::standard()).unwrap();
        // make a vec of OpjectPositionInsert
        let new_element = ObjectPositionInsert {
            calculated_at_ms: now_utc,
            date: date.to_string(),
            lat_sector,
            lon_sector,
            data_chunk: encoded,
        };

        let q = diesel::insert_into(objectposition::table)
            .values(new_element)
            //.returning(ObjectPositionStored::as_returning())
            .execute(conn)
            .expect("error saving");
        println!("Inserted {} rows.", q);
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
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::app_settings)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct AppSettingsUpsert {
    pub id: i32,
    pub lat: f64,
    pub lon: f64,
    pub view_windows_json: String,
}

impl AppSettingsRow {
    pub fn into_settings(self) -> Result<AppSettings, String> {
        AppSettings::from_parts(self.lat, self.lon, &self.view_windows_json)
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
                view_windows_json TEXT NOT NULL
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
