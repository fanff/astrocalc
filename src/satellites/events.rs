//! Serializable ISS prediction events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::propagate::Topocentric;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TrackSample {
    pub utc: DateTime<Utc>,
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
    pub range_km: f64,
}

impl TrackSample {
    pub fn from_topo(utc: DateTime<Utc>, topo: Topocentric) -> Self {
        Self {
            utc,
            altitude_deg: topo.altitude_deg,
            azimuth_deg: topo.azimuth_deg,
            range_km: topo.range_km,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VisiblePass {
    pub aos: DateTime<Utc>,
    pub los: DateTime<Utc>,
    pub peak: DateTime<Utc>,
    pub max_altitude_deg: f64,
    pub peak_azimuth_deg: f64,
    /// True when ISS was sunlit and sky dark through the listed window.
    pub illuminated: bool,
    /// Visible-window length (sunlit + dark sky + above min alt), seconds.
    #[serde(default)]
    pub duration_secs: i64,
    /// Sun–ISS–observer phase angle at peak (degrees).
    #[serde(default)]
    pub phase_angle_deg: f64,
    /// Estimated apparent magnitude at peak (range + phase + airmass).
    #[serde(default)]
    pub peak_magnitude: f64,
    /// Slant range at peak (km).
    #[serde(default)]
    pub peak_range_km: f64,
    pub track: Vec<TrackSample>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiskBody {
    Sun,
    Moon,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DiskTransit {
    pub body: DiskBody,
    pub center_time: DateTime<Utc>,
    /// Minimum angular separation ISS↔disk center (degrees).
    pub min_separation_deg: f64,
    /// Body semi-diameter at event (degrees).
    pub semi_diameter_deg: f64,
    /// True when ISS crosses the disk (`min_separation <= semi_diameter`).
    pub is_transit: bool,
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
    pub range_km: f64,
    /// Approximate ground miss distance from center-line (km); `None` if unknown.
    pub centerline_miss_km: Option<f64>,
    /// Suggested ground move toward the center-line (km); for near-misses.
    #[serde(default)]
    pub move_hint_km: Option<f64>,
    /// Compass bearing of that move in degrees from north toward east (`0` = north).
    #[serde(default)]
    pub move_hint_bearing_deg: Option<f64>,
    /// Moon illuminated fraction percent when `body == Moon`; unused for Sun.
    #[serde(default)]
    pub moon_illum_pct: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn visible_pass_json_roundtrip() {
        let p = VisiblePass {
            aos: Utc.with_ymd_and_hms(2006, 6, 16, 20, 0, 0).unwrap(),
            los: Utc.with_ymd_and_hms(2006, 6, 16, 20, 6, 0).unwrap(),
            peak: Utc.with_ymd_and_hms(2006, 6, 16, 20, 3, 0).unwrap(),
            max_altitude_deg: 42.0,
            peak_azimuth_deg: 180.0,
            illuminated: true,
            duration_secs: 360,
            phase_angle_deg: 45.0,
            peak_magnitude: -1.5,
            peak_range_km: 500.0,
            track: vec![TrackSample {
                utc: Utc.with_ymd_and_hms(2006, 6, 16, 20, 3, 0).unwrap(),
                altitude_deg: 42.0,
                azimuth_deg: 180.0,
                range_km: 500.0,
            }],
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: VisiblePass = serde_json::from_str(&s).unwrap();
        assert_eq!(back, p);
    }
}
