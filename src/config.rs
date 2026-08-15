//! Domain settings: observer location and visibility zones.
//! Persistence lives in SQLite (`app_settings`); this module owns types and validation.

use core::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Copy, Debug)]
pub struct ViewWindow {
    pub min_az_deg: f64,
    pub max_az_deg: f64,
    pub min_alt_deg: f64,
    pub max_alt_deg: f64,
}

impl ViewWindow {
    /// Default zone for "Add zone" (eastish wedge, mid altitudes).
    pub fn default_zone() -> Self {
        Self::new(60.0, 120.0, 10.0, 45.0)
    }

    /// Near-full sky looking north (wraps across north; small southern blind spot).
    pub fn paris_north_almost_360() -> Self {
        Self::new(185.0, 175.0, 5.0, 80.0)
    }

    /// Create a new ViewWindow
    pub fn new(min_az: f64, max_az: f64, min_alt: f64, max_alt: f64) -> Self {
        Self {
            min_az_deg: normalize_az(min_az),
            max_az_deg: normalize_az(max_az),
            min_alt_deg: min_alt,
            max_alt_deg: max_alt,
        }
    }

    /// True when the azimuth range crosses north (`min_az > max_az`).
    pub fn wraps_north(&self) -> bool {
        self.min_az_deg > self.max_az_deg
    }

    pub fn az_span_deg(&self) -> f64 {
        if self.wraps_north() {
            (360.0 - self.min_az_deg) + self.max_az_deg
        } else {
            self.max_az_deg - self.min_az_deg
        }
    }

    pub fn is_valid(&self) -> bool {
        let az_ok = self.min_az_deg >= 0.0
            && self.min_az_deg <= 360.0
            && self.max_az_deg >= 0.0
            && self.max_az_deg <= 360.0
            && self.az_span_deg() > 1e-6
            && !(self.min_az_deg == self.max_az_deg);
        let alt_ok = self.min_alt_deg >= 0.0
            && self.max_alt_deg <= 90.0
            && self.min_alt_deg < self.max_alt_deg;
        az_ok && alt_ok
    }

    pub fn contains(&self, az_deg: f64, alt_deg: f64) -> bool {
        if alt_deg < self.min_alt_deg || alt_deg > self.max_alt_deg {
            return false;
        }
        let az = normalize_az(az_deg);
        if self.wraps_north() {
            az >= self.min_az_deg || az <= self.max_az_deg
        } else {
            az >= self.min_az_deg && az <= self.max_az_deg
        }
    }

    /// Ensure alt bounds stay ordered and within [0, 90] with a minimum span.
    pub fn clamp_alts(&mut self, min_span: f64) {
        self.min_alt_deg = self.min_alt_deg.clamp(0.0, 90.0 - min_span);
        self.max_alt_deg = self.max_alt_deg.clamp(min_span, 90.0);
        if self.max_alt_deg - self.min_alt_deg < min_span {
            if self.max_alt_deg >= 90.0 - 1e-9 {
                self.min_alt_deg = (self.max_alt_deg - min_span).max(0.0);
            } else {
                self.max_alt_deg = (self.min_alt_deg + min_span).min(90.0);
            }
        }
    }
}

/// Normalize degrees into `[0, 360)`.
pub fn normalize_az(az_deg: f64) -> f64 {
    let mut a = az_deg % 360.0;
    if a < 0.0 {
        a += 360.0;
    }
    if a >= 360.0 {
        a = 0.0;
    }
    a
}

/// Human‑readable representation of a `ViewWindow`.
impl fmt::Display for ViewWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.wraps_north() {
            write!(
                f,
                "Azimuth: [{:.1}° → 360°/0° → {:.1}°] (wraps N)\nAltitude: [{:.1}°, {:.1}°]",
                self.min_az_deg, self.max_az_deg, self.min_alt_deg, self.max_alt_deg
            )
        } else {
            write!(
                f,
                "Azimuth: [{:.1}°, {:.1}°]\nAltitude: [{:.1}°, {:.1}°]",
                self.min_az_deg, self.max_az_deg, self.min_alt_deg, self.max_alt_deg
            )
        }
    }
}

/// Durable observer + view settings (persisted in SQLite `app_settings`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AppSettings {
    pub lat: f64,
    pub lon: f64,
    pub view_windows: Vec<ViewWindow>,
    /// Bortle dark-sky class 1 (excellent) … 9 (inner city). Default 5 (suburban).
    #[serde(default = "default_bortle_class")]
    pub bortle_class: u8,
}

fn default_bortle_class() -> u8 {
    5
}

impl AppSettings {
    /// Paris center, looking almost 360° across north.
    pub fn paris_defaults() -> Self {
        Self {
            lat: 48.8566,
            lon: 2.3522,
            view_windows: vec![ViewWindow::paris_north_almost_360()],
            bortle_class: 5,
        }
    }

    pub fn is_valid(&self) -> bool {
        if self.lat < -90.0 || self.lat > 90.0 {
            return false;
        }
        if self.lon < -180.0 || self.lon > 180.0 {
            return false;
        }
        if !(1..=9).contains(&self.bortle_class) {
            return false;
        }
        if self.view_windows.is_empty() {
            return false;
        }
        self.view_windows.iter().all(|vw| vw.is_valid())
    }

    pub fn view_windows_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.view_windows)
    }

    pub fn from_parts(
        lat: f64,
        lon: f64,
        view_windows_json: &str,
        bortle_class: u8,
    ) -> Result<Self, String> {
        let view_windows: Vec<ViewWindow> = serde_json::from_str(view_windows_json)
            .map_err(|e| format!("invalid view_windows_json: {e}"))?;
        Ok(Self {
            lat,
            lon,
            view_windows,
            bortle_class: bortle_class.clamp(1, 9),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paris_defaults_are_valid() {
        let s = AppSettings::paris_defaults();
        assert!(s.is_valid());
        assert!((s.lat - 48.8566).abs() < 1e-9);
        assert!((s.lon - 2.3522).abs() < 1e-9);
        assert_eq!(s.view_windows.len(), 1);
        let vw = s.view_windows[0];
        assert!(vw.wraps_north());
        assert!((vw.az_span_deg() - 350.0).abs() < 1e-9);
    }

    #[test]
    fn paris_north_zone_contains_north_excludes_south() {
        let vw = ViewWindow::paris_north_almost_360();
        assert!(vw.contains(0.0, 40.0));
        assert!(vw.contains(10.0, 20.0));
        assert!(vw.contains(350.0, 20.0));
        assert!(!vw.contains(180.0, 40.0));
        assert!(!vw.contains(0.0, 2.0));
        assert!(!vw.contains(0.0, 85.0));
    }

    #[test]
    fn view_windows_json_round_trip() {
        let s = AppSettings::paris_defaults();
        let json = s.view_windows_json().unwrap();
        let restored = AppSettings::from_parts(s.lat, s.lon, &json, s.bortle_class).unwrap();
        assert_eq!(s, restored);
    }

    #[test]
    fn invalid_empty_windows() {
        let s = AppSettings {
            lat: 48.0,
            lon: 2.0,
            view_windows: vec![],
            bortle_class: 5,
        };
        assert!(!s.is_valid());
    }
}
