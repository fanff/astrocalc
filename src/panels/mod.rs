pub mod config;
pub mod dailysolar;
pub mod iss;
pub mod longterm_plot;
pub mod longterm_timeline;

#[derive(Clone, Copy, Debug)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}
impl LatLon {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
    pub fn snap(&self, precision: i32) -> LatLon {
        let factor = 10f64.powi(precision as i32);
        let snap = |v: f64| (v * factor).round() / factor;
        LatLon {
            lat: snap(self.lat),
            lon: snap(self.lon),
        }
    }
}
