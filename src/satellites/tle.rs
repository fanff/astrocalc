//! ISS TLE fetch (Celestrak) and on-disk cache.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use super::propagate::Propagator;

pub const ISS_NORAD_ID: u64 = 25544;

fn celestrak_iss_tle_url() -> String {
    format!("https://celestrak.org/NORAD/elements/gp.php?CATNR={ISS_NORAD_ID}&FORMAT=tle")
}

/// Default max age before a background refetch is preferred.
pub const DEFAULT_TLE_FRESHNESS: Duration = Duration::hours(6);
/// Warn / treat transit predictions as soft when TLE older than this.
pub const TRANSIT_WARN_AGE: Duration = Duration::hours(12);
/// Visible-pass predictions become unreliable beyond this age.
pub const PASS_STALE_AGE: Duration = Duration::hours(24);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CachedTle {
    pub name: String,
    pub line1: String,
    pub line2: String,
    pub fetched_at: DateTime<Utc>,
    /// Epoch from the TLE itself.
    pub tle_epoch: DateTime<Utc>,
}

impl CachedTle {
    pub fn age(&self, now: DateTime<Utc>) -> Duration {
        now.signed_duration_since(self.fetched_at)
    }

    pub fn epoch_age(&self, now: DateTime<Utc>) -> Duration {
        now.signed_duration_since(self.tle_epoch)
    }

    pub fn is_fresh(&self, now: DateTime<Utc>, freshness: Duration) -> bool {
        self.age(now) < freshness
    }

    pub fn propagator(&self) -> Result<Propagator, String> {
        Propagator::from_tle(Some(&self.name), &self.line1, &self.line2)
    }
}

#[derive(Clone, Debug)]
pub struct TleCache {
    path: PathBuf,
    freshness: Duration,
}

impl TleCache {
    pub fn new(cache_dir: impl AsRef<Path>, freshness: Duration) -> Result<Self, std::io::Error> {
        let dir = cache_dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join("iss_tle.json"),
            freshness,
        })
    }

    pub fn load(&self) -> Option<CachedTle> {
        let text = fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn store(&self, cached: &CachedTle) -> Result<(), String> {
        let text =
            serde_json::to_string_pretty(cached).map_err(|e| format!("serialize TLE: {e}"))?;
        fs::write(&self.path, text).map_err(|e| format!("write TLE cache: {e}"))
    }

    /// Return cached TLE if fresh enough; otherwise fetch and store.
    pub fn get_or_fetch(&self, force: bool) -> Result<CachedTle, String> {
        let now = Utc::now();
        if !force {
            if let Some(c) = self.load() {
                if c.is_fresh(now, self.freshness) {
                    return Ok(c);
                }
            }
        }
        let fetched = fetch_iss_tle()?;
        self.store(&fetched)?;
        Ok(fetched)
    }
}

/// Parse a 2- or 3-line TLE text block into `CachedTle`.
pub fn parse_tle_text(text: &str, fetched_at: DateTime<Utc>) -> Result<CachedTle, String> {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() < 2 {
        return Err("TLE text too short".into());
    }
    let (name, l1, l2) = if lines[0].starts_with('1') && lines.len() >= 2 {
        ("ISS (ZARYA)".to_string(), lines[0], lines[1])
    } else if lines.len() >= 3 && lines[1].starts_with('1') {
        (lines[0].to_string(), lines[1], lines[2])
    } else {
        return Err(format!("unrecognized TLE layout ({} lines)", lines.len()));
    };
    let prop = Propagator::from_tle(Some(&name), l1, l2)?;
    Ok(CachedTle {
        name,
        line1: l1.to_string(),
        line2: l2.to_string(),
        fetched_at,
        tle_epoch: prop.epoch_utc(),
    })
}

/// HTTP fetch from Celestrak (blocking; call from async Bind / background).
pub fn fetch_iss_tle() -> Result<CachedTle, String> {
    let body = ureq::get(&celestrak_iss_tle_url())
        .call()
        .map_err(|e| format!("TLE HTTP: {e}"))?
        .into_string()
        .map_err(|e| format!("TLE body: {e}"))?;
    parse_tle_text(&body, Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::satellites::fixtures;
    use chrono::TimeZone;

    #[test]
    fn parse_sample_tle() {
        let fetched = Utc.with_ymd_and_hms(2008, 9, 20, 12, 0, 0).unwrap();
        let c = parse_tle_text(fixtures::ISS_TLE_TEXT, fetched).unwrap();
        assert!(c.name.contains("ISS"));
        assert!(c.line1.starts_with('1'));
        assert!(c.line2.starts_with('2'));
        assert_eq!(c.fetched_at, fetched);
        let prop = c.propagator().unwrap();
        assert_eq!(prop.elements().norad_id, ISS_NORAD_ID);
    }

    #[test]
    fn cache_roundtrip_file() {
        let dir = std::env::temp_dir().join(format!("astrocalc_tle_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let cache = TleCache::new(&dir, Duration::hours(6)).unwrap();
        let fetched = Utc.with_ymd_and_hms(2008, 9, 20, 12, 0, 0).unwrap();
        let c = parse_tle_text(fixtures::ISS_TLE_TEXT, fetched).unwrap();
        cache.store(&c).unwrap();
        let loaded = cache.load().unwrap();
        assert_eq!(loaded, c);
        let _ = fs::remove_dir_all(&dir);
    }
}
