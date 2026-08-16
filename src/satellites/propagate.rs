//! SGP4 propagation and TEME → topocentric az/alt/range.

use chrono::{DateTime, NaiveDateTime, Utc};
use sgp4::{Constants, Elements, MinutesSinceEpoch, Prediction};

use julian_day_converter::unixtime_to_julian_day;

/// WGS-84 equatorial radius (km).
pub const WGS84_A_KM: f64 = 6378.137;
/// WGS-84 flattening.
pub const WGS84_F: f64 = 1.0 / 298.257_223_563;
/// Mean Earth radius used for cylindrical umbra (km).
pub const EARTH_RADIUS_MEAN_KM: f64 = 6371.0;

#[derive(Clone, Copy, Debug)]
pub struct Observer {
    pub lat_deg: f64,
    pub lon_deg: f64,
    /// Height above ellipsoid (km); usually 0.
    pub height_km: f64,
}

impl Observer {
    pub fn new(lat_deg: f64, lon_deg: f64) -> Self {
        Self {
            lat_deg,
            lon_deg,
            height_km: 0.0,
        }
    }

    /// Geodetic → ECEF (km).
    pub fn ecef_km(&self) -> [f64; 3] {
        let lat = self.lat_deg.to_radians();
        let lon = self.lon_deg.to_radians();
        let sin_lat = lat.sin();
        let cos_lat = lat.cos();
        let e2 = WGS84_F * (2.0 - WGS84_F);
        let n = WGS84_A_KM / (1.0 - e2 * sin_lat * sin_lat).sqrt();
        let x = (n + self.height_km) * cos_lat * lon.cos();
        let y = (n + self.height_km) * cos_lat * lon.sin();
        let z = (n * (1.0 - e2) + self.height_km) * sin_lat;
        [x, y, z]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Topocentric {
    /// Altitude above horizon (degrees).
    pub altitude_deg: f64,
    /// Azimuth from north toward east, `[0, 360)` (matches solar-system pipeline).
    pub azimuth_deg: f64,
    /// Slant range (km).
    pub range_km: f64,
}

#[derive(Clone, Debug)]
pub struct Propagator {
    elements: Elements,
    constants: Constants,
}

impl Propagator {
    pub fn from_elements(elements: Elements) -> Result<Self, sgp4::ElementsError> {
        let constants = Constants::from_elements(&elements)?;
        Ok(Self {
            elements,
            constants,
        })
    }

    pub fn from_tle(name: Option<&str>, line1: &str, line2: &str) -> Result<Self, String> {
        let elements = Elements::from_tle(
            name.map(|s| s.to_owned()),
            line1.as_bytes(),
            line2.as_bytes(),
        )
        .map_err(|e| format!("TLE parse: {e:?}"))?;
        Self::from_elements(elements).map_err(|e| format!("SGP4 constants: {e:?}"))
    }

    pub fn elements(&self) -> &Elements {
        &self.elements
    }

    pub fn epoch_utc(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_naive_utc_and_offset(self.elements.datetime, Utc)
    }

    /// TEME position (km) at `utc`.
    pub fn teme_km_at(&self, utc: DateTime<Utc>) -> Result<[f64; 3], String> {
        let pred = self.predict_at(utc)?;
        Ok(pred.position)
    }

    pub fn predict_at(&self, utc: DateTime<Utc>) -> Result<Prediction, String> {
        let naive: NaiveDateTime = utc.naive_utc();
        let minutes = self
            .elements
            .datetime_to_minutes_since_epoch(&naive)
            .map_err(|e| format!("minutes since epoch: {e:?}"))?;
        self.constants
            .propagate(minutes)
            .map_err(|e| format!("SGP4 propagate: {e:?}"))
    }

    pub fn predict_at_minutes(&self, minutes: f64) -> Result<Prediction, String> {
        self.constants
            .propagate(MinutesSinceEpoch(minutes))
            .map_err(|e| format!("SGP4 propagate: {e:?}"))
    }

    /// Topocentric look angles from `observer` at `utc`.
    pub fn observe(&self, observer: &Observer, utc: DateTime<Utc>) -> Result<Topocentric, String> {
        let pred = self.predict_at(utc)?;
        let ecef = teme_to_ecef(pred.position, utc);
        Ok(ecef_to_topocentric(observer, ecef))
    }
}

/// Approximate TEME → ECEF via GMST rotation about Z (visual-satellite accuracy).
pub fn teme_to_ecef(teme_km: [f64; 3], utc: DateTime<Utc>) -> [f64; 3] {
    let jd = unixtime_to_julian_day(utc.timestamp());
    let gmst = astro::time::mn_sidr(jd);
    let (s, c) = gmst.sin_cos();
    let [x, y, z] = teme_km;
    [c * x + s * y, -s * x + c * y, z]
}

/// ECEF satellite position → topocentric az/alt/range (north-based azimuth).
pub fn ecef_to_topocentric(observer: &Observer, sat_ecef_km: [f64; 3]) -> Topocentric {
    let obs = observer.ecef_km();
    let rho = [
        sat_ecef_km[0] - obs[0],
        sat_ecef_km[1] - obs[1],
        sat_ecef_km[2] - obs[2],
    ];
    let range = (rho[0] * rho[0] + rho[1] * rho[1] + rho[2] * rho[2]).sqrt();

    let lat = observer.lat_deg.to_radians();
    let lon = observer.lon_deg.to_radians();
    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lon, cos_lon) = lon.sin_cos();

    // South, East, Zenith local frame.
    let south = -sin_lat * cos_lon * rho[0] - sin_lat * sin_lon * rho[1] + cos_lat * rho[2];
    let east = -sin_lon * rho[0] + cos_lon * rho[1];
    let zenith = cos_lat * cos_lon * rho[0] + cos_lat * sin_lon * rho[1] + sin_lat * rho[2];

    let altitude_deg = (zenith / range).asin().to_degrees();
    // North = -south; az from north toward east.
    let azimuth_deg = {
        let mut az = east.atan2(-south).to_degrees();
        if az < 0.0 {
            az += 360.0;
        }
        az
    };

    Topocentric {
        altitude_deg,
        azimuth_deg,
        range_km: range,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::satellites::fixtures;
    use chrono::TimeZone;

    #[test]
    fn parse_and_propagate_at_epoch() {
        let prop = Propagator::from_tle(
            Some("ISS (ZARYA)"),
            fixtures::ISS_TLE_L1,
            fixtures::ISS_TLE_L2,
        )
        .unwrap();
        let pred = prop.predict_at_minutes(0.0).unwrap();
        let r =
            (pred.position[0].powi(2) + pred.position[1].powi(2) + pred.position[2].powi(2)).sqrt();
        assert!(
            (6500.0..7200.0).contains(&r),
            "unexpected geocentric range {r} km {:?}",
            pred.position
        );
    }

    #[test]
    fn topocentric_overhead_near_subsatellite() {
        let prop =
            Propagator::from_tle(Some("ISS"), fixtures::ISS_TLE_L1, fixtures::ISS_TLE_L2).unwrap();
        let utc = prop.epoch_utc();
        let teme = prop.teme_km_at(utc).unwrap();
        let ecef = teme_to_ecef(teme, utc);
        // Sub-satellite point roughly from ECEF direction.
        let r = (ecef[0].powi(2) + ecef[1].powi(2) + ecef[2].powi(2)).sqrt();
        let lat = (ecef[2] / r).asin().to_degrees();
        let lon = ecef[1].atan2(ecef[0]).to_degrees();
        let obs = Observer::new(lat, lon);
        let topo = ecef_to_topocentric(&obs, ecef);
        assert!(
            topo.altitude_deg > 80.0,
            "expected near zenith, got alt={}",
            topo.altitude_deg
        );
        assert!(topo.range_km < 500.0, "range {}", topo.range_km);
    }

    #[test]
    fn observe_below_horizon_far_away() {
        let prop =
            Propagator::from_tle(Some("ISS"), fixtures::ISS_TLE_L1, fixtures::ISS_TLE_L2).unwrap();
        // Antipode-ish observer relative to epoch subpoint → usually below horizon.
        let utc = Utc.with_ymd_and_hms(2008, 9, 20, 12, 0, 0).unwrap();
        let topo = prop.observe(&Observer::new(-40.0, 140.0), utc).unwrap();
        assert!(topo.altitude_deg < 30.0, "alt={}", topo.altitude_deg);
    }
}
