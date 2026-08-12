use std::str::FromStr;

use crate::{
    config::{AppSettings, ViewWindow},
    panels::LatLon,
    panels::config::ConfigPanel,
    panels::dailysolar::DailySolar,
    panels::longterm_plot::LongTermPlot,
    timezone_util::site_tz_from_lat_lon,
    weather_cache::{Location, WeatherCache, WeatherRequest, WeatherSnapshot, noon_utc_for_date},
    widgets::{
        location_map::LocationMap,
        solarcalc::solar_calc_button,
        view_window_editor::ViewWindowEditorState,
    },
};
use eframe::egui;
use egui::{Color32, Context, Frame, Ui};
use chrono::{DateTime, NaiveDate, offset::Utc};
use egui_async::{Bind, EguiAsyncPlugin};
use egui_extras::DatePickerButton;

// This struct holds the data (state) for our application.
pub struct AstroCalcApp {
    pub panel_view: usize,
    pub start_date: NaiveDate,
    pub day_count: i64,
    pub freq_minutes: i64,
    pub location_map: LocationMap,
    pub lat: f64,
    pub long: f64,
    pub view_windows: Vec<ViewWindow>,
    pub zone_editor: ViewWindowEditorState,

    pub solar_calculation_trigger: Bind<String, String>,
    pub weather_cache: WeatherCache,
    pub weather_bind: Bind<WeatherSnapshot, String>,
    pub weather_req_key: Option<(NaiveDate, i64, i64)>,

    pub selected_output_tz: String,
    pub selected_output_tz_obj: chrono_tz::Tz,

    pub long_term_data: LongTermPlot,
    pub daily_solar_data: DailySolar,
    pub database_url: String,
}

impl AstroCalcApp {
    pub fn new(
        egui_ctx: Context,
        settings: AppSettings,
        database_url: String,
        weather_cache: WeatherCache,
    ) -> Self {
        let lat = settings.lat;
        let long = settings.lon;
        let site_tz = site_tz_from_lat_lon(lat, long);
        let mut app = Self {
            panel_view: 0,
            start_date: Utc::now().date_naive(),
            day_count: 10,
            freq_minutes: 10,
            location_map: LocationMap::new(egui_ctx, long, lat),
            lat,
            long,
            view_windows: settings.view_windows,
            zone_editor: ViewWindowEditorState::default(),
            solar_calculation_trigger: Bind::new(false),
            weather_cache,
            weather_bind: Bind::new(true),
            weather_req_key: None,
            selected_output_tz: site_tz.name().to_string(),
            selected_output_tz_obj: site_tz,
            long_term_data: LongTermPlot::new(LatLon::new(lat, long)),
            daily_solar_data: DailySolar::new(
                Utc::now().date_naive(),
                LatLon::new(lat, long),
                vec![],
                database_url.clone(),
            ),
            database_url,
        };
        app.daily_solar_data.set_local_tz(site_tz);
        app
    }
}

impl AstroCalcApp {
    fn pub_panel_view(&mut self, ui: &mut Ui) {
        let prev = self.panel_view;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.panel_view, 0, "Config");
            ui.selectable_value(&mut self.panel_view, 1, "Solar System");
            ui.selectable_value(&mut self.panel_view, 2, "Long Term");
            ui.selectable_value(&mut self.panel_view, 3, "Daily");
        });
        if self.panel_view == 3 && prev != 3 {
            self.daily_solar_data.lat_lon = LatLon::new(self.lat, self.long);
            self.sync_site_timezone_from_map();
            self.daily_solar_data.refresh_positions();
        }
        ui.separator();
    }

    fn config_panel_view(&mut self, ui: &mut Ui) {
        let changed = ConfigPanel {
            lat: &mut self.lat,
            long: &mut self.long,
            timezone_name: self.selected_output_tz_obj.name(),
            database_url: self.database_url.as_str(),
            view_windows: &mut self.view_windows,
            zone_editor: &mut self.zone_editor,
            location_map: &mut self.location_map,
        }
        .show(ui);
        if changed {
            self.sync_site_timezone_from_map();
        }
    }

    pub fn calculation_panel_view(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("Start Date:");
                let date_pick = DatePickerButton::new(&mut self.start_date);
                ui.add(date_pick);

                ui.label("Calculation Period (Days):");
                ui.add(egui::Slider::new(&mut self.day_count, 1..=120).text("Days"));
                ui.label("Calculation Frequency (Minutes):");
                ui.add(egui::Slider::new(&mut self.freq_minutes, 1..=60).text("Minutes"));
            });
            ui.vertical(|ui| {
                ui.label("Select Objects:");
                egui::ComboBox::from_label("")
                    .selected_text(format!("{}", self.selected_output_tz))
                    .show_ui(ui, |ui| {
                        chrono_tz::TZ_VARIANTS.iter().for_each(|tz| {
                            let sv = ui.selectable_value(
                                &mut self.selected_output_tz,
                                tz.name().to_owned(),
                                tz.name(),
                            );
                            if sv.clicked() {
                                let tz = chrono_tz::Tz::from_str(tz.name()).ok().unwrap();
                                self.selected_output_tz_obj = tz;
                                println!("Selected timezone: {:?}", tz);
                            }
                        });
                    });
            });
        });
        solar_calc_button(
            ui,
            &mut self.solar_calculation_trigger,
            LatLon {
                lat: self.lat,
                lon: self.long,
            },
            self.start_date,
            self.freq_minutes,
            self.day_count,
            Some(self.database_url.clone()),
        );

        if let Some(res) = self.solar_calculation_trigger.read() {
            match res {
                Err(e) => {
                    ui.colored_label(Color32::RED, format!("Error: {}", e));
                }
                Ok(_) => {
                    ui.label("got it !");
                }
            }
        }
    }

    fn sync_site_timezone_from_map(&mut self) {
        let tz = site_tz_from_lat_lon(self.lat, self.long);
        self.selected_output_tz_obj = tz;
        self.selected_output_tz = tz.name().to_string();
        self.daily_solar_data.set_local_tz(tz);
    }

    fn weather_target_time(&self) -> DateTime<Utc> {
        if let Some(n) = self.daily_solar_data.dateinfo.as_ref() {
            let start = n.night_start_ms;
            let end = n.night_end_ms;
            if end > start {
                return start + (end - start) / 2;
            }
        }
        noon_utc_for_date(self.daily_solar_data.date)
    }

    fn weather_key(&self) -> (NaiveDate, i64, i64) {
        let snapped = self.weather_cache.snap_location(Location {
            lat: self.daily_solar_data.lat_lon.lat,
            lon: self.daily_solar_data.lat_lon.lon,
        });
        (
            self.daily_solar_data.date,
            (snapped.lat * 100.0).round() as i64,
            (snapped.lon * 100.0).round() as i64,
        )
    }

    fn refresh_weather_if_needed(&mut self) {
        let key = self.weather_key();
        if self.weather_req_key.as_ref() == Some(&key) {
            return;
        }
        self.start_weather_fetch(false);
    }

    fn force_weather_refresh(&mut self) {
        self.start_weather_fetch(true);
    }

    fn start_weather_fetch(&mut self, force: bool) {
        if self.weather_bind.is_pending() {
            return;
        }
        self.weather_req_key = Some(self.weather_key());
        let cache = self.weather_cache.clone();
        let req = WeatherRequest {
            location: Location {
                lat: self.daily_solar_data.lat_lon.lat,
                lon: self.daily_solar_data.lat_lon.lon,
            },
            target_time: self.weather_target_time(),
        };
        self.weather_bind
            .refresh(async move { cache.get_weather(&req, force).await });
    }

    fn sync_weather_into_daily(&mut self) {
        let pending = self.weather_bind.is_pending();
        self.daily_solar_data.weather_pending = pending;
        if pending {
            self.daily_solar_data.weather_error = None;
            return;
        }
        match self.weather_bind.read() {
            Some(Ok(snap)) => {
                self.daily_solar_data.weather_snapshot = Some(snap.clone());
                self.daily_solar_data.weather_error = None;
            }
            Some(Err(e)) => {
                self.daily_solar_data.weather_error = Some(e.clone());
            }
            None => {}
        }
    }
}
impl eframe::App for AstroCalcApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        ctx.plugin_or_default::<EguiAsyncPlugin>();

        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ctx, |ui| {
                self.pub_panel_view(ui);
                if self.panel_view == 0 {
                    self.config_panel_view(ui);
                }
                if self.panel_view == 1 {
                    self.calculation_panel_view(ui);
                }
                if self.panel_view == 2 {
                    ui.add(&mut self.long_term_data);
                }
                if self.panel_view == 3 {
                    self.daily_solar_data.lat_lon = LatLon::new(self.lat, self.long);
                    self.daily_solar_data.set_local_tz(self.selected_output_tz_obj);
                    self.daily_solar_data.view_windows = self.view_windows.clone();
                    self.refresh_weather_if_needed();
                    self.sync_weather_into_daily();
                    ui.add(&mut self.daily_solar_data);
                    if self.daily_solar_data.weather_force_refresh {
                        self.daily_solar_data.weather_force_refresh = false;
                        self.force_weather_refresh();
                        self.sync_weather_into_daily();
                    }
                }
            });
    }
}
