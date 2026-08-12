// Solar system calculations: planet positions, moon position, night intervals

use core::fmt;
use std::collections::HashMap;

use astro::{
    angle, consts,
    coords::{self, EclPoint, hr_angl_frm_observer_long},
    ecliptic,
    lunar::{self, geocent_ecl_pos},
    planet::{self, Planet},
    sun,
    time::{self, CalType, Date, DayOfMonth},
};
use bincode::{Decode, Encode};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};

use diesel::{Connection, SqliteConnection};
use egui::Color32;
use julian_day_converter::unixtime_to_julian_day;

use polars::{
    error::{PolarsError, PolarsResult},
    frame::{DataFrame, UniqueKeepStrategy},
    prelude::{
        DataType, IntoLazy, NamedFromOwned, SortMultipleOptions, SortOptions, col, lit, when,
    },
    series::{ChunkCompareEq, Series, SeriesTrait},
};
use serde::{Deserialize, Serialize};
use sunrise_sunset_calculator::SunriseSunsetParameters;

use crate::{
    models::{DateInfo, DateInfoInsert, ObjectPositionInsert, ObjectPositionStored},
    panels::LatLon,
    panels::dailysolar::is_in_viewwindow,
};
const AU_IN_KM: f64 = 149_597_870.7;

// Planets (including Pluto if you wish)
pub const PLANET_LIST: [planet::Planet; 7] = [
    planet::Planet::Mercury,
    planet::Planet::Venus,
    planet::Planet::Mars,
    planet::Planet::Jupiter,
    planet::Planet::Saturn,
    planet::Planet::Uranus,
    planet::Planet::Neptune,
];
pub const PLANET_NAMES: [&str; 7] = [
    "Mercury", "Venus", "Mars", "Jupiter", "Saturn", "Uranus", "Neptune",
];
pub const OBJECT_NAMES_WITH_MOON: [&str; 8] = [
    "Mercury", "Venus", "Mars", "Jupiter", "Saturn", "Uranus", "Neptune", "Moon",
];

/// Bright, conventional colors readable on a dark plot background.
static OBJECT_COLORS: [Color32; 8] = [
    Color32::from_rgb(180, 180, 190), // Mercury - silver-gray
    Color32::from_rgb(255, 236, 140), // Venus - bright pale gold
    Color32::from_rgb(255, 90, 60),   // Mars - vivid orange-red
    Color32::from_rgb(255, 190, 110), // Jupiter - warm amber
    Color32::from_rgb(230, 210, 150), // Saturn - pale butter-gold
    Color32::from_rgb(90, 220, 230),  // Uranus - cyan
    Color32::from_rgb(80, 160, 255),  // Neptune - bright azure
    Color32::from_rgb(235, 240, 250), // Moon - soft white
];

pub fn get_object_color(object_name: &str) -> Color32 {
    OBJECT_NAME_TO_INDEX
        .get(object_name)
        .map(|&i| OBJECT_COLORS[i])
        .unwrap_or_else(|| dso_color_from_name(object_name))
}

/// Stable, readable color for deep-sky (and other unknown) object names.
fn dso_color_from_name(name: &str) -> Color32 {
    let mut hash: u32 = 2166136261;
    for b in name.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(16777619);
    }
    let h = (hash % 360) as f32;
    // Soft HSV → RGB for dark backgrounds.
    let s = 0.55_f32;
    let v = 0.85_f32;
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Darken a line color for Gantt bar fill while keeping hue readable under white text.
pub fn darken_for_bar_fill(color: Color32) -> Color32 {
    let factor = 0.38;
    Color32::from_rgb(
        (color.r() as f32 * factor) as u8,
        (color.g() as f32 * factor) as u8,
        (color.b() as f32 * factor) as u8,
    )
}

// make a static hash map of planet name to index in Planet Name
pub const OBJECT_NAME_TO_INDEX: phf::Map<&'static str, usize> = phf::phf_map! {
    "Mercury" => 0,
    "Venus" => 1,
    "Mars" => 2,
    "Jupiter" => 3,
    "Saturn" => 4,
    "Uranus" => 5,
    "Neptune" => 6,
    "Moon" => 7,
};

/// Compute apparent sky positions (alt/az) and magnitudes for all planets
pub fn solar_system_positions(
    date: NaiveDate,
    datetime: DateTime<Utc>,
    lat_deg: f64,
    lon_deg: f64,
) -> Vec<ObjectPosition> {
    // Convert UTC datetime → Julian day
    let jd = unixtime_to_julian_day(datetime.timestamp());
    let gmst = time::mn_sidr(jd);

    //println!("{:?} -> jd: {:?} gmst:{:?}",datetime,jd, gmst);
    // Observer latitude & longitude in radians
    let lat_rad = lat_deg.to_radians();
    let lon_rad = lon_deg.to_radians();
    // mean obliquity of the ecliptic (radians)
    let eps = ecliptic::mn_oblq_IAU(jd);

    PLANET_LIST
        .iter()
        .zip(&PLANET_NAMES)
        .filter_map(|(p, pname)| {
            // Computes a planet's geocentric, apparent ecliptic position, corrected for light-time
            let (ecl_point, rad_vec) = planet::geocent_apprnt_ecl_coords(&p, jd);

            let (ra, dec) = ecl_point_to_radec(&ecl_point, eps);

            // normalize RA to [0, 2π)
            let ra_norm = angle::limit_to_two_PI(ra);

            // Convert RA/Dec → horizontal coordinates (Az/Alt)
            let hour_angle = hr_angl_frm_observer_long(gmst, -lon_rad, ra);
            let (az, alt) = astro::loc_hz_frm_eq!(hour_angle, dec, lat_rad);

            let (mag, delta) = calc_mag(p, jd);

            Some(ObjectPosition {
                name: pname.to_string(),
                utc_datetime: datetime,
                date,
                ra: ra_norm.to_degrees(),
                dec: dec.to_degrees(),
                altitude: alt.to_degrees(),
                azimuth: angle::limit_to_360(az.to_degrees() + 180.0),
                magnitude: mag,
                distance: delta,
                phase_ratio: 0.0,
            })
        })
        .collect()
}

pub fn moon_position(
    date: NaiveDate,
    datetime: DateTime<Utc>,
    lat_deg: f64,
    lon_deg: f64,
) -> ObjectPosition {
    // Convert UTC datetime → Julian day
    let jd = unixtime_to_julian_day(datetime.timestamp());
    let gmst = time::mn_sidr(jd);

    let lat_rad = lat_deg.to_radians();
    let lon_rad = lon_deg.to_radians();

    // mean obliquity of the ecliptic (radians)
    let eps = ecliptic::mn_oblq_IAU(jd);

    let (moon_ecl_point, earth_moon_dist) = lunar::geocent_ecl_pos(jd);

    //println!("{:?},{:?}",moon_ecl_point.lat,moon_ecl_point.long);

    let (ra, dec) = ecl_point_to_radec(&moon_ecl_point, eps);

    let (sun_ecl_point, sun_earth_dist) = sun::geocent_ecl_pos(jd);
    let moon_fract = lunar::illum_frac_frm_ecl_coords(
        moon_ecl_point.long,
        moon_ecl_point.lat,
        sun_ecl_point.long,
        earth_moon_dist,
        sun_earth_dist * AU_IN_KM,
    );

    let hour_angle = hr_angl_frm_observer_long(gmst, -lon_rad, ra);
    let (az, alt) = astro::loc_hz_frm_eq!(hour_angle, dec, lat_rad);

    //println!("{:?},{:?} , {}",angle::hms_frm_deg(ra.to_degrees()),
    //angle::dms_frm_deg(dec.to_degrees()), moon_fract);
    ObjectPosition {
        name: "Moon".to_string(),
        utc_datetime: datetime,
        date: date,
        ra: ra.to_degrees(),
        dec: dec.to_degrees(),
        altitude: alt.to_degrees(),
        azimuth: angle::limit_to_360(az.to_degrees() + 180.0),
        magnitude: 0.0,
        distance: earth_moon_dist,
        phase_ratio: moon_fract * 100.0,
    }
}
// ----- rounding helpers (UTC) -----
fn floor_to_minutes(dt: DateTime<Utc>, step_min: i64) -> DateTime<Utc> {
    let step = step_min * 60; // seconds
    let ts = dt.timestamp();
    let rem = ts.rem_euclid(step);
    DateTime::<Utc>::from_timestamp(ts - rem, 0).unwrap()
}

fn ceil_to_minutes(dt: DateTime<Utc>, step_min: i64) -> DateTime<Utc> {
    let step = step_min * 60; // seconds
    let ts = dt.timestamp();
    let rem = ts.rem_euclid(step);
    if rem == 0 {
        dt
    } else {
        DateTime::<Utc>::from_timestamp(ts + (step - rem), 0).unwrap()
    }
}

pub fn nights_spans(start_date: NaiveDate, lat: f64, lon: f64, day_count: i64) -> Vec<NightInfo> {
    // compute one extra day for "next sunrise"
    let mut daily: Vec<(NaiveDate, i64, i64)> = Vec::new();
    // convert lon to h m s
    let lon_h = (lon / 15.0).floor() as i32;
    let lon_m = ((lon / 15.0 - lon_h as f64) * 60.0).floor() as i32;
    for day_offset in 0..=day_count {
        let current_day = start_date + Duration::days(day_offset);
        let ts = current_day
            .and_time(NaiveTime::from_hms_opt((12 - lon_h) as u32, 0, 0).unwrap())
            .and_utc()
            .timestamp();

        let result = SunriseSunsetParameters::new(ts, lat, lon)
            .calculate()
            .expect("sun calc failed");

        daily.push((current_day, result.rise, result.set));
    }
    println!("Computed daily spans: {:?}", daily);

    // (day, ceil_10m(sunset), floor_10m(next_sunrise))
    let mut nights = Vec::with_capacity(daily.len().saturating_sub(1));
    for ((day, _rise, set), (_next_day, next_rise, _next_set)) in
        daily.iter().zip(daily.iter().skip(1))
    {
        let set_dt = DateTime::<Utc>::from_timestamp(*set, 0).unwrap();
        let next_rise_dt = DateTime::<Utc>::from_timestamp(*next_rise, 0).unwrap();

        let set_dt_ceil10 = ceil_to_minutes(set_dt, 10);
        let next_rise_dt_floor10 = floor_to_minutes(next_rise_dt, 10);

        nights.push(NightInfo {
            date: *day,
            night_start_ms: set_dt_ceil10,
            night_end_ms: next_rise_dt_floor10,
        });
    }
    println!("Computed nights spans: {:?}", nights);
    nights
}

pub fn build_night_intervals(
    nights: &[NightInfo],
    interval_minutes: i64,
) -> Vec<(NaiveDate, Vec<DateTime<Utc>>)> {
    let step = Duration::minutes(interval_minutes);

    nights
        .iter()
        .map(|ni| {
            // Safety: we expect next_sunrise >= sunset (since it's the next day).
            // If not, return an empty vector for that entry.
            if ni.night_end_ms < ni.night_start_ms {
                return (ni.date, Vec::new());
            }

            let mut ticks = Vec::new();
            let mut t = ni.night_start_ms;

            // Include both ends; push at start, then step until we've passed the end.
            while t <= ni.night_end_ms {
                ticks.push(t);
                t += step;
            }

            (ni.date, ticks)
        })
        .collect()
}

pub fn sph_to_cart(lon: f64, lat: f64, r: f64) -> (f64, f64, f64) {
    let (slon, clon) = lon.sin_cos();
    let (slat, clat) = lat.sin_cos();
    let x = r * clat * clon;
    let y = r * clat * slon;
    let z = r * slat;
    (x, y, z)
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct NightInfo {
    pub date: NaiveDate,
    pub night_start_ms: DateTime<Utc>,
    pub night_end_ms: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ObjectPosition {
    pub name: String,
    pub utc_datetime: DateTime<Utc>,
    pub date: NaiveDate,
    pub ra: f64,
    pub dec: f64,

    pub altitude: f64,
    pub azimuth: f64,

    pub magnitude: f64,
    pub distance: f64,

    pub phase_ratio: f64,
}
impl Encode for ObjectPosition {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> core::result::Result<(), bincode::error::EncodeError> {
        bincode::Encode::encode(&self.name, encoder)?;
        bincode::Encode::encode(&self.utc_datetime.timestamp_millis(), encoder)?;
        bincode::Encode::encode(&self.date.to_string(), encoder)?;
        bincode::Encode::encode(&self.ra, encoder)?;
        bincode::Encode::encode(&self.dec, encoder)?;
        bincode::Encode::encode(&self.altitude, encoder)?;
        bincode::Encode::encode(&self.azimuth, encoder)?;
        bincode::Encode::encode(&self.magnitude, encoder)?;
        bincode::Encode::encode(&self.distance, encoder)?;
        bincode::Encode::encode(&self.phase_ratio, encoder)?;

        Ok(())
    }
}
impl<Context> Decode<Context> for ObjectPosition {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> core::result::Result<Self, bincode::error::DecodeError> {
        let some_name: String = bincode::Decode::decode(decoder).unwrap();
        let timestamp_millis: i64 = bincode::Decode::decode(decoder).unwrap();
        let date_str: String = bincode::Decode::decode(decoder).unwrap();
        Ok(Self {
            name: some_name,
            utc_datetime: DateTime::<Utc>::from_timestamp_millis(timestamp_millis).unwrap(),
            date: NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").unwrap(),
            ra: bincode::Decode::decode(decoder)?,
            dec: bincode::Decode::decode(decoder)?,
            altitude: bincode::Decode::decode(decoder)?,
            azimuth: bincode::Decode::decode(decoder)?,
            magnitude: bincode::Decode::decode(decoder)?,
            distance: bincode::Decode::decode(decoder)?,
            phase_ratio: bincode::Decode::decode(decoder)?,
        })
    }
}

// at unit test for encode / decode
#[cfg(test)]
mod tests_object_position {
    use super::*;
    #[test]
    fn test_object_position_encode_decode() {
        let obj_pos = ObjectPosition {
            name: "Mars".to_string(),
            utc_datetime: DateTime::from_timestamp_millis(Utc::now().timestamp_millis()).unwrap(),
            date: Utc::now().date_naive(),
            ra: 123.45,
            dec: -54.32,
            altitude: 30.0,
            azimuth: 150.0,
            magnitude: -1.5,
            distance: 0.5,
            phase_ratio: 75.0,
        };

        let encoded = bincode::encode_to_vec(&obj_pos, bincode::config::standard()).unwrap();
        let (decoded, _): (ObjectPosition, usize) =
            bincode::decode_from_slice(&encoded, bincode::config::standard()).unwrap();

        assert_eq!(obj_pos, decoded);
    }
}

impl fmt::Display for ObjectPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (h, m, s) = angle::hms_frm_deg(self.ra);
        let (dec_d, dec_m, dec_s) = angle::dms_frm_deg(self.dec);
        let (az_d, az_m, az_s) = angle::dms_frm_deg(self.azimuth);
        let (alt_d, alt_m, alt_s) = angle::dms_frm_deg(self.altitude);
        write!(
            f,
            "{}:\n  Mag   : {}\n  Dist  : {}\n  Ra/Dec: {}h{}m{}s {}°{}\'{:.1}\"\n  Az/Alt: {}°{}\'{:.1}\" {}°{}\'{:.1}\"\n  Phase:  {:.0}%",
            self.name,
            self.magnitude,
            self.distance,
            h,
            m,
            s,
            dec_d,
            dec_m.abs(),
            dec_s.abs(),
            az_d,
            az_m.abs(),
            az_s.abs(),
            alt_d,
            alt_m.abs(),
            alt_s.abs(),
            self.phase_ratio
        )
    }
}

#[derive(Clone, Debug)]
pub struct ObjectSegment(Vec<ObjectPosition>);
impl ObjectSegment {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, ObjectPosition> {
        self.0.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, ObjectPosition> {
        self.0.iter_mut()
    }
}

#[derive(Clone, Debug)]
pub struct ObjectPositionSegments {
    pub segments: HashMap<String, Vec<ObjectSegment>>,
}
impl ObjectPositionSegments {
    pub fn new() -> Self {
        Self {
            segments: HashMap::new(),
        }
    }
    pub fn from_positions(positions: &[ObjectPosition], max_segment_gap_minutes: i64) -> Self {
        let mut segments_map: HashMap<String, Vec<ObjectSegment>> = HashMap::new();

        for pos in positions {
            let entry = segments_map
                .entry(pos.name.clone())
                .or_insert_with(Vec::new);

            if let Some(last_segment) = entry.last_mut() {
                let ls = &mut last_segment.0;
                let last_pos = ls.last().unwrap();
                let time_diff = pos
                    .utc_datetime
                    .signed_duration_since(last_pos.utc_datetime);
                if time_diff.num_minutes() <= max_segment_gap_minutes {
                    ls.push(pos.clone());
                } else {
                    entry.push(ObjectSegment(vec![pos.clone()]));
                }
            } else {
                entry.push(ObjectSegment(vec![pos.clone()]));
            }
        }

        Self {
            segments: segments_map,
        }
    }
    pub fn filter_view(
        &self,
        view_windows: &Vec<crate::config::ViewWindow>,
        min_duration_minutes: i64,
        selected_object_names: &Vec<String>,
    ) -> ObjectPositionSegments {
        let mut filtered_segments_map: HashMap<String, Vec<ObjectSegment>> = HashMap::new();

        for (object_name, segments) in &self.segments {
            if !selected_object_names.contains(object_name) {
                continue;
            }
            let mut filtered_segments = Vec::new();

            for segment in segments {
                // filter positions in segment by view windows
                let filtered_positions: Vec<ObjectPosition> = segment
                    .0
                    .iter()
                    .filter(|pos| is_in_viewwindow(pos, view_windows))
                    .cloned()
                    .collect();

                if !filtered_positions.is_empty() {
                    // sort filtered positions by utc_datetime
                    let mut filtered_positions = filtered_positions;
                    filtered_positions.sort_by_key(|pos| pos.utc_datetime);
                    // get first and last position utc_datetime
                    let first_datetime = filtered_positions.first().map(|pos| pos.utc_datetime);
                    let last_datetime = filtered_positions.last().map(|pos| pos.utc_datetime);
                    // calculate duration in minutes
                    let duration_minutes =
                        if let (Some(first), Some(last)) = (first_datetime, last_datetime) {
                            last.signed_duration_since(first).num_minutes()
                        } else {
                            0
                        };
                    if duration_minutes > min_duration_minutes {
                        filtered_segments.push(ObjectSegment(filtered_positions));
                    }
                }
            }

            if !filtered_segments.is_empty() {
                filtered_segments_map.insert(object_name.clone(), filtered_segments);
            }
        }

        ObjectPositionSegments {
            segments: filtered_segments_map,
        }
    }
}

pub fn calc_mag(p: &Planet, jd: f64) -> (f64, f64) {
    // calculate magnitude

    // `p_long`   : Heliocentric longitude *| in radians*
    // `p_lat`    : Heliocentric latitude *| in radians*
    // `p_rad_vec`: Heliocentric radius vector *| in AU*
    let (p_long, p_lat, p_rad_vec) = planet::heliocent_coords(p, jd);

    // Earth heliocentric coords
    let (e_long, e_lat, e_rad_vec) = planet::heliocent_coords(&planet::Planet::Earth, jd);

    let (px, py, pz) = sph_to_cart(p_long, p_lat, p_rad_vec);
    let (ex, ey, ez) = sph_to_cart(e_long, e_lat, e_rad_vec);

    // Geocentric vector (planet relative to Earth) and distance Δ (AU)
    let dx = px - ex;
    let dy = py - ey;
    let dz = pz - ez;
    let delta = (dx * dx + dy * dy + dz * dz).sqrt();
    // Sun–planet distance r (AU)
    let r = p_rad_vec;

    // Phase angle i (Sun–planet–Earth).
    // cos(i) = (r^2 - P·E) / (r * Δ), where P and E are heliocentric vectors.
    let dot_pe = px * ex + py * ey + pz * ez;
    let cos_i = ((r * r) - dot_pe) / (r * delta);
    let i = cos_i.clamp(-1.0, 1.0).acos();

    // Compute apparent magnitude (G. Müller formula)
    //i : Phase angle of the planet | in radians
    //delta : Planet-Earth distance | in AU
    //r : Planet-Sun distance | in AU
    let mag_res = planet::apprnt_mag_muller(p, i, delta, r);
    let mut mag = 0.0;
    if mag_res.is_ok() {
        mag = mag_res.unwrap();
    }
    (mag, delta)
}

pub fn ecl_point_to_radec(ecl_point: &EclPoint, eps: f64) -> (f64, f64) {
    let sin_lambda = ecl_point.long.sin();
    let cos_lambda = ecl_point.long.cos();
    let sin_beta = ecl_point.lat.sin();
    let cos_beta = ecl_point.lat.cos();
    let sin_eps = eps.sin();
    let cos_eps = eps.cos();

    let ra = f64::atan2(
        sin_lambda * cos_eps - (sin_beta / cos_beta) * sin_eps,
        cos_lambda,
    );
    let dec = (sin_beta * cos_eps + cos_beta * sin_eps * sin_lambda).asin();

    (ra, dec)
}

pub fn calculate_solar_system_positions(
    start_date: NaiveDate,
    lat: f64,
    long: f64,
    freq_minutes: i64,
    day_count: i64,
    database_url: Option<String>,
) -> (Vec<ObjectPosition>, Vec<NightInfo>) {
    let snapped_lat_lon = LatLon { lat, lon: long }.snap(2);
    let mut nights = nights_spans(start_date.clone(), lat, long, day_count);
    let do_store: bool = database_url.is_some();
    let url = database_url.unwrap_or("".to_string());
    if do_store {
        let mut cofnn = SqliteConnection::establish(url.as_str()).unwrap();
        // get available dates from NightInfo
        let in_db = DateInfo::available_dates_from_db(&mut cofnn, &snapped_lat_lon);

        let all_insert: Vec<&NightInfo> = nights
            .iter()
            .filter(|ni| !in_db.contains(&ni.date))
            .collect();
        DateInfoInsert::insert_from_vec(
            &mut cofnn,
            &all_insert,
            snapped_lat_lon.lat,
            snapped_lat_lon.lon,
        );
    }
    if do_store {
        let mut conn = SqliteConnection::establish(url.as_str()).unwrap();
        let days = ObjectPositionStored::available_days(&mut conn, &snapped_lat_lon);
        println!("Days requested count : {:?}", nights.len());
        nights = nights
            .iter()
            .filter(|ni| !days.contains(&ni.date))
            .cloned()
            .collect();
        println!("Days to calculate  count : {:?}", nights.len());
    }
    let expanded: Vec<(NaiveDate, Vec<DateTime<Utc>>)> =
        build_night_intervals(&nights, freq_minutes);

    let mut object_positions = Vec::new();
    let mut object_positions_buff = Vec::new();
    // store_position in db here

    for (date, r) in expanded {
        object_positions_buff.clear();
        for dt in r {
            let planet_pos = solar_system_positions(date, dt, lat, long);
            object_positions.extend(planet_pos.clone());
            object_positions_buff.extend(planet_pos);

            let moon_pos = moon_position(date, dt, lat, long);
            object_positions.push(moon_pos.clone());
            object_positions_buff.push(moon_pos);
        }
        if do_store {
            let mut conn: SqliteConnection = SqliteConnection::establish(url.as_str()).unwrap();
            //
            ObjectPositionInsert::insert_date(
                &mut conn,
                date,
                snapped_lat_lon.lat,
                snapped_lat_lon.lon,
                &object_positions_buff,
            );
        }
        println!(
            "Calculated {} objects for date {:?}",
            object_positions.len(),
            date
        );
    }

    (object_positions, nights)
}
