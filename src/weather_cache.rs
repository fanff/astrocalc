use chrono::{DateTime, Duration, NaiveDateTime, TimeDelta, Timelike, Utc};
use open_meteo_rs::forecast::{ForecastResult, ForecastResultHourly};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, DirEntry},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherRequest {
    pub location: Location,
    /// The time you want a forecast for (used to pick night-relevant hours in the UI).
    pub target_time: DateTime<Utc>,
}

/// What we store in the YAML cache file (API payload as YAML text).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastData {
    pub yaml_content: String,
}

/// On-disk cache record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedForecast {
    pub location: Location,
    pub fetched_at: DateTime<Utc>,
    pub data: ForecastData,
}

/// One hourly sample extracted for the UI.
#[derive(Debug, Clone)]
pub struct HourlyWeatherPoint {
    pub datetime: DateTime<Utc>,
    pub cloud_cover: Option<f64>,
    pub humidity: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub temperature: Option<f64>,
    pub visibility: Option<f64>,
}

/// Structured forecast ready for the UI (no chart drawing here).
#[derive(Debug, Clone)]
pub struct WeatherSnapshot {
    pub snapped: Location,
    pub fetched_at: DateTime<Utc>,
    pub hourly: Vec<HourlyWeatherPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherCache {
    cache_dir: PathBuf,
    /// Max age of a cached entry before we force a refetch
    freshness: Duration,
    /// How many decimal places to keep in lat/lon for the cache key
    /// e.g. 2 decimals ≈ ~1 km "close enough"
    coord_precision: u32,
}

impl WeatherCache {
    /// Create a cache directory named `app_name` under the process CWD.
    pub fn new(
        app_name: &str,
        freshness: Duration,
        coord_precision: u32,
    ) -> Result<Self, std::io::Error> {
        let cache_dir = PathBuf::from(app_name);
        fs::create_dir_all(&cache_dir)?;

        Ok(Self {
            cache_dir,
            freshness,
            coord_precision,
        })
    }

    pub fn snap_location(&self, loc: Location) -> Location {
        let factor = 10f64.powi(self.coord_precision as i32);
        let snap = |v: f64| (v * factor).round() / factor;
        Location {
            lat: snap(loc.lat),
            lon: snap(loc.lon),
        }
    }

    /// Main entry point: get weather from cache if possible, otherwise API.
    /// When `force` is true, skips the freshness cache and always hits the API.
    pub async fn get_weather(
        &self,
        req: &WeatherRequest,
        force: bool,
    ) -> Result<WeatherSnapshot, String> {
        if !force {
            if let Some(cached) = self.try_get_from_cache(req)? {
                println!("Using cached weather data.");
                return Self::snapshot_from_cached(cached);
            }
        }
        println!("Fetching weather data from API...");
        let (forecast, fetched_at) = self.fetch_from_api(req).await?;
        let snapped = self.snap_location(req.location);
        let yaml = serde_yaml::to_string(&forecast).map_err(|e| e.to_string())?;
        let data = ForecastData {
            yaml_content: yaml,
        };
        let cached = CachedForecast {
            location: snapped,
            fetched_at,
            data: data.clone(),
        };
        self.save_to_cache(&cached)?;
        Ok(WeatherSnapshot {
            snapped,
            fetched_at,
            hourly: hourly_from_forecast(&forecast),
        })
    }

    fn cache_file_path(&self, loc: Location, fetched_at: DateTime<Utc>) -> PathBuf {
        let file_name = format!(
            "{:.2}_{:.2}_{:.0}.yaml",
            loc.lat,
            loc.lon,
            fetched_at.timestamp()
        );
        self.cache_dir.join(file_name)
    }

    fn try_get_from_cache(&self, req: &WeatherRequest) -> Result<Option<CachedForecast>, String> {
        let snapped = self.snap_location(req.location);
        println!(
            "Looking for cache file for location ({:.2}, {:.2})",
            snapped.lat, snapped.lon
        );
        let re = Regex::new(&format!(
            r"^{:.2}_{:.2}_(\d+)\.yaml$",
            snapped.lat, snapped.lon
        ))
        .map_err(|e| e.to_string())?;

        let candidates = fs::read_dir(&self.cache_dir)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                let caps = re.captures(&file_name_str)?;
                let timestamp = caps[1].parse::<i64>().ok()?;
                let file_time = DateTime::from_timestamp(timestamp, 0)?;
                let file_freshness = Utc::now() - file_time;
                if file_freshness > TimeDelta::zero() && file_freshness <= self.freshness {
                    Some((entry, file_freshness))
                } else {
                    None
                }
            });

        let mut sorted_candidates: Vec<(DirEntry, Duration)> = candidates.collect();
        sorted_candidates.sort_by_key(|k| k.1);
        println!("Cache candidates: {:?}", &sorted_candidates);

        if let Some((entry, _)) = sorted_candidates.first() {
            let text = fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
            let cached: CachedForecast =
                serde_yaml::from_str(&text).map_err(|e| e.to_string())?;
            Ok(Some(cached))
        } else {
            Ok(None)
        }
    }

    fn save_to_cache(&self, cached: &CachedForecast) -> Result<(), String> {
        let path = self.cache_file_path(cached.location, cached.fetched_at);
        let yaml = serde_yaml::to_string(cached).map_err(|e| e.to_string())?;
        fs::write(&path, yaml).map_err(|e| e.to_string())
    }

    async fn fetch_from_api(
        &self,
        req: &WeatherRequest,
    ) -> Result<(ForecastResult, DateTime<Utc>), String> {
        let client = open_meteo_rs::Client::new();
        let mut opts = open_meteo_rs::forecast::Options::default();

        opts.location = open_meteo_rs::Location {
            lat: req.location.lat,
            lng: req.location.lon,
        };
        opts.forecast_days = Some(5);
        opts.time_zone = Some("UTC".into());
        opts.hourly.push("temperature_2m".into());
        opts.hourly.push("cloud_cover".into());
        opts.hourly.push("visibility".into());
        opts.hourly.push("relative_humidity_2m".into());
        opts.hourly.push("wind_speed_10m".into());
        opts.hourly.push("wind_direction_10m".into());

        let res = client
            .forecast(opts)
            .await
            .map_err(|e| format!("Open-Meteo fetch failed: {e}"))?;
        Ok((res, Utc::now()))
    }

    fn snapshot_from_cached(cached: CachedForecast) -> Result<WeatherSnapshot, String> {
        let forecast: ForecastResult =
            serde_yaml::from_str(&cached.data.yaml_content).map_err(|e| e.to_string())?;
        Ok(WeatherSnapshot {
            snapped: cached.location,
            fetched_at: cached.fetched_at,
            hourly: hourly_from_forecast(&forecast),
        })
    }
}

fn hourly_from_forecast(forecast: &ForecastResult) -> Vec<HourlyWeatherPoint> {
    let Some(hourly) = forecast.hourly.as_ref() else {
        return Vec::new();
    };
    hourly.iter().map(point_from_hourly).collect()
}

fn point_from_hourly(h: &ForecastResultHourly) -> HourlyWeatherPoint {
    HourlyWeatherPoint {
        datetime: naive_as_utc(h.datetime),
        cloud_cover: value_f64(&h.values, "cloud_cover"),
        humidity: value_f64(&h.values, "relative_humidity_2m"),
        wind_speed: value_f64(&h.values, "wind_speed_10m"),
        wind_direction: value_f64(&h.values, "wind_direction_10m"),
        temperature: value_f64(&h.values, "temperature_2m"),
        visibility: value_f64(&h.values, "visibility"),
    }
}

fn naive_as_utc(dt: NaiveDateTime) -> DateTime<Utc> {
    DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)
}

fn value_f64(
    values: &std::collections::HashMap<String, open_meteo_rs::forecast::ForecastResultItem>,
    key: &str,
) -> Option<f64> {
    let item = values.get(key)?;
    match &item.value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Compact wind direction label from degrees (meteorological: direction wind comes from).
pub fn wind_cardinal(degrees: f64) -> &'static str {
    let dirs = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    let idx = ((degrees.rem_euclid(360.0) + 11.25) / 22.5) as usize % dirs.len();
    dirs[idx]
}

impl WeatherSnapshot {
    /// Hours overlapping `[start, end]` (inclusive-ish on start).
    pub fn night_hours(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<&HourlyWeatherPoint> {
        self.hourly
            .iter()
            .filter(|h| h.datetime >= start && h.datetime <= end)
            .collect()
    }

    pub fn mean_opt(values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            None
        } else {
            Some(values.iter().sum::<f64>() / values.len() as f64)
        }
    }

    pub fn max_opt(values: &[f64]) -> Option<f64> {
        values.iter().copied().reduce(f64::max)
    }
}

/// Midpoint of a UTC day when night bounds are unknown.
pub fn noon_utc_for_date(date: chrono::NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(12, 0, 0)
        .map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc))
        .unwrap_or_else(Utc::now)
}

/// Round age for status labels.
pub fn format_age(fetched_at: DateTime<Utc>) -> String {
    let age = Utc::now() - fetched_at;
    let mins = age.num_minutes().max(0);
    if mins < 1 {
        "just now".into()
    } else if mins < 60 {
        format!("{mins} min ago")
    } else {
        format!("{} h ago", mins / 60)
    }
}

/// Format an hour label in UTC.
pub fn format_hour_utc(dt: DateTime<Utc>) -> String {
    format!("{:02}:00", dt.hour())
}
