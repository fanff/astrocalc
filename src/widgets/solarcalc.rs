use egui_async::Bind;

use crate::{panels::LatLon, solarsystemcalc::calculate_solar_system_positions};

pub fn solar_calc_button(
    ui: &mut egui::Ui,
    on: &mut Bind<String, String>,
    lat_lon: LatLon,
    start_date: chrono::NaiveDate,
    freq_minutes: i64,
    day_count: i64,
    database_url: Option<String>,
) -> egui::Response {
    if on.is_pending() {
        ui.label("Calculating...");
        ui.spinner();
    } else {
        if ui.button("Calc").clicked() {
            on.refresh(async move {
                calculate_solar_system_positions(
                    start_date,
                    lat_lon.lat,
                    lat_lon.lon,
                    freq_minutes,
                    day_count,
                    database_url,
                );
                Ok("done".into())
                //Err("Not implemented".into())
            });
        }
    }
    ui.response()
}
