use crate::{
    config::{AppSettings, ViewWindow},
    deepsky::ensure_dso_batch,
    panels::LatLon,
    panels::config::ConfigPanel,
    panels::dailysolar::{DAILY_PREFETCH_DAY_COUNT, DailySolar, SAMPLE_FREQ_MINUTES},
    panels::longterm_plot::LongTermPlot,
    solarsystemcalc::calculate_solar_system_positions,
    timezone_util::site_tz_from_lat_lon,
    weather_cache::{Location, WeatherCache, WeatherRequest, WeatherSnapshot, noon_utc_for_date},
    widgets::{
        location_map::LocationMap, view_window_editor::ViewWindowEditorState,
    },
};
use chrono::{DateTime, NaiveDate, offset::Utc};
use eframe::egui;
use egui::{Context, Frame, Ui};
use egui_async::{Bind, EguiAsyncPlugin};

pub struct AstroCalcApp {
    pub panel_view: usize,
    pub location_map: LocationMap,
    pub lat: f64,
    pub long: f64,
    pub view_windows: Vec<ViewWindow>,
    pub zone_editor: ViewWindowEditorState,

    pub ephemeris_bind: Bind<(), String>,
    /// Key for the in-flight or last-started Daily ephemeris job.
    pub ephemeris_req_key: Option<(NaiveDate, i64, i64)>,
    /// Last key for which Daily ephemeris Bind completed successfully.
    pub ephemeris_done_key: Option<(NaiveDate, i64, i64)>,
    /// Long Term pan-triggered prefetch (batches of 10 nights).
    pub long_term_bind: Bind<(), String>,
    pub long_term_req_key: Option<(NaiveDate, i64, i64)>,
    pub long_term_done_key: Option<(NaiveDate, i64, i64)>,
    /// Long Term DSO backfill (batches of 10 nights for newly selected catalog objects).
    pub long_term_dso_bind: Bind<(), String>,
    pub long_term_dso_batch_key: Option<(NaiveDate, usize, i64, i64)>,
    pub long_term_dso_done_key: Option<(NaiveDate, usize, i64, i64)>,
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
            location_map: LocationMap::new(egui_ctx, long, lat),
            lat,
            long,
            view_windows: settings.view_windows,
            zone_editor: ViewWindowEditorState::default(),
            ephemeris_bind: Bind::new(false),
            ephemeris_req_key: None,
            ephemeris_done_key: None,
            long_term_bind: Bind::new(false),
            long_term_req_key: None,
            long_term_done_key: None,
            long_term_dso_bind: Bind::new(false),
            long_term_dso_batch_key: None,
            long_term_dso_done_key: None,
            weather_cache,
            weather_bind: Bind::new(true),
            weather_req_key: None,
            selected_output_tz: site_tz.name().to_string(),
            selected_output_tz_obj: site_tz,
            long_term_data: LongTermPlot::new(LatLon::new(lat, long), database_url.clone()),
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
            ui.selectable_value(&mut self.panel_view, 1, "Long Term");
            ui.selectable_value(&mut self.panel_view, 2, "Daily");
        });
        if self.panel_view == 2 && prev != 2 {
            self.daily_solar_data.lat_lon = LatLon::new(self.lat, self.long);
            self.sync_site_timezone_from_map();
            self.daily_solar_data.refresh_positions();
        }
        if self.panel_view == 1 && prev != 1 {
            self.long_term_data.lat_lon = LatLon::new(self.lat, self.long);
            self.long_term_data.view_windows = self.view_windows.clone();
            self.long_term_data.refresh_from_db();
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
            self.daily_solar_data.lat_lon = LatLon::new(self.lat, self.long);
            self.long_term_data.lat_lon = LatLon::new(self.lat, self.long);
            self.ephemeris_req_key = None;
            self.ephemeris_done_key = None;
            self.long_term_req_key = None;
            self.long_term_done_key = None;
            self.long_term_dso_batch_key = None;
            self.long_term_dso_done_key = None;
            self.daily_solar_data.refresh_positions();
            self.long_term_data.view_windows = self.view_windows.clone();
            self.long_term_data.refresh_from_db();
        }
    }

    fn sync_site_timezone_from_map(&mut self) {
        let tz = site_tz_from_lat_lon(self.lat, self.long);
        self.selected_output_tz_obj = tz;
        self.selected_output_tz = tz.name().to_string();
        self.daily_solar_data.set_local_tz(tz);
    }

    fn ephemeris_key(&self) -> (NaiveDate, i64, i64) {
        let snapped = self.daily_solar_data.lat_lon.snap(2);
        (
            self.daily_solar_data.date,
            (snapped.lat * 100.0).round() as i64,
            (snapped.lon * 100.0).round() as i64,
        )
    }

    fn long_term_prefetch_key(&self, start: NaiveDate) -> (NaiveDate, i64, i64) {
        let snapped = self.long_term_data.lat_lon.snap(2);
        (
            start,
            (snapped.lat * 100.0).round() as i64,
            (snapped.lon * 100.0).round() as i64,
        )
    }

    fn ensure_long_term_prefetch(&mut self) {
        let pending = self.long_term_bind.is_pending();
        self.long_term_data.ephemeris_pending = pending;

        if pending {
            return;
        }

        if let Some(start) = self.long_term_data.prefetch_start {
            let key = self.long_term_prefetch_key(start);
            if self.long_term_req_key == Some(key) && self.long_term_done_key != Some(key) {
                if let Some(res) = self.long_term_bind.read() {
                    if res.is_ok() {
                        self.long_term_done_key = Some(key);
                        self.long_term_data.reload_after_prefetch();
                    } else {
                        self.long_term_data.prefetch_start = None;
                    }
                    return;
                }
            }

            if self.long_term_done_key == Some(key) {
                self.long_term_data.prefetch_start = None;
                return;
            }

            self.long_term_req_key = Some(key);
            let snapped = self.long_term_data.lat_lon.snap(2);
            let db = self.database_url.clone();
            self.long_term_data.ephemeris_pending = true;
            self.long_term_bind.refresh(async move {
                calculate_solar_system_positions(
                    start,
                    snapped.lat,
                    snapped.lon,
                    SAMPLE_FREQ_MINUTES,
                    DAILY_PREFETCH_DAY_COUNT,
                    Some(db),
                );
                Ok(())
            });
        }
    }

    fn ensure_long_term_dso_backfill(&mut self) {
        let pending = self.long_term_dso_bind.is_pending();
        self.long_term_data.dso_backfill_pending = pending;

        if pending {
            return;
        }

        // Consume completed batch.
        if let Some(key) = self.long_term_dso_batch_key {
            if self.long_term_dso_done_key != Some(key) {
                if let Some(res) = self.long_term_dso_bind.read() {
                    if res.is_ok() {
                        self.long_term_dso_done_key = Some(key);
                        self.long_term_data.reload_after_dso_backfill();
                    }
                    self.long_term_dso_batch_key = None;
                }
            }
        }

        if self.long_term_data.dso_backfill_queue.is_empty() {
            self.long_term_data.dso_backfill_pending = false;
            return;
        }

        let batch = self
            .long_term_data
            .take_dso_backfill_batch(DAILY_PREFETCH_DAY_COUNT as usize);
        if batch.is_empty() {
            return;
        }

        let snapped = self.long_term_data.lat_lon.snap(2);
        let start = batch[0];
        let key = (
            start,
            batch.len(),
            (snapped.lat * 100.0).round() as i64,
            (snapped.lon * 100.0).round() as i64,
        );
        self.long_term_dso_batch_key = Some(key);
        self.long_term_dso_done_key = None;

        let db = self.database_url.clone();
        let ids = self.long_term_data.catalog_select.selected_dso_ids();
        let lat = snapped.lat;
        let lon = snapped.lon;
        self.long_term_data.dso_backfill_pending = true;
        self.long_term_dso_bind.refresh(async move {
            ensure_dso_batch(
                &db,
                lat,
                lon,
                SAMPLE_FREQ_MINUTES,
                &ids,
                &batch,
            );
            Ok(())
        });
    }

    fn ensure_ephemeris_prefetch(&mut self) {
        let key = self.ephemeris_key();
        let pending = self.ephemeris_bind.is_pending();
        self.daily_solar_data.ephemeris_pending = pending;

        if pending {
            return;
        }

        // Consume a completed job for the current key.
        if self.ephemeris_req_key == Some(key) && self.ephemeris_done_key != Some(key) {
            if let Some(res) = self.ephemeris_bind.read() {
                if res.is_ok() {
                    self.ephemeris_done_key = Some(key);
                    self.daily_solar_data.reload_cached_only();
                    self.daily_solar_data.request_ephemeris_prefetch = false;
                    if self.panel_view == 1 {
                        self.long_term_data.refresh_from_db();
                    }
                } else {
                    self.daily_solar_data.request_ephemeris_prefetch = false;
                }
                return;
            }
        }

        if !self.daily_solar_data.request_ephemeris_prefetch {
            return;
        }

        // Solar window already filled for this date/sector.
        if self.ephemeris_done_key == Some(key) {
            self.daily_solar_data.request_ephemeris_prefetch = false;
            return;
        }

        self.ephemeris_req_key = Some(key);
        let date = self.daily_solar_data.date;
        let snapped = self.daily_solar_data.lat_lon.snap(2);
        let db = self.database_url.clone();
        self.daily_solar_data.ephemeris_pending = true;
        self.ephemeris_bind.refresh(async move {
            calculate_solar_system_positions(
                date,
                snapped.lat,
                snapped.lon,
                SAMPLE_FREQ_MINUTES,
                DAILY_PREFETCH_DAY_COUNT,
                Some(db),
            );
            Ok(())
        });
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
                    self.long_term_data.lat_lon = LatLon::new(self.lat, self.long);
                    self.long_term_data.local_tz = self.selected_output_tz_obj;
                    self.long_term_data.view_windows = self.view_windows.clone();
                    self.ensure_long_term_prefetch();
                    self.ensure_long_term_dso_backfill();
                    ui.add(&mut self.long_term_data);
                    self.ensure_long_term_prefetch();
                    self.ensure_long_term_dso_backfill();
                    if let Some(date) = self.long_term_data.goto_daily_date.take() {
                        self.panel_view = 2;
                        self.daily_solar_data.date = date;
                        self.daily_solar_data.lat_lon = LatLon::new(self.lat, self.long);
                        self.daily_solar_data.set_local_tz(self.selected_output_tz_obj);
                        self.daily_solar_data.view_windows = self.view_windows.clone();
                        self.daily_solar_data.refresh_positions();
                    }
                }
                if self.panel_view == 2 {
                    self.daily_solar_data.lat_lon = LatLon::new(self.lat, self.long);
                    self.daily_solar_data.set_local_tz(self.selected_output_tz_obj);
                    self.daily_solar_data.view_windows = self.view_windows.clone();
                    self.ensure_ephemeris_prefetch();
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
