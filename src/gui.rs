use crate::{
    config::{AppSettings, ViewWindow},
    deepsky::ensure_dso_batch,
    models::{IssEventInsert, IssEventRow},
    panels::LatLon,
    panels::config::ConfigPanel,
    panels::dailysolar::{DAILY_PREFETCH_DAY_COUNT, DailySolar, SAMPLE_FREQ_MINUTES},
    panels::iss::IssPanelState,
    panels::longterm_plot::LongTermPlot,
    satellites::{ISS_PREDICT_DAY_COUNT, IssPredictionBundle, TleCache, fetch_and_predict},
    solarsystemcalc::calculate_solar_system_positions,
    timezone_util::site_tz_from_lat_lon,
    weather_cache::{Location, WeatherCache, WeatherRequest, WeatherSnapshot, noon_utc_for_date},
    widgets::{location_map::LocationMap, view_window_editor::ViewWindowEditorState},
};
use chrono::{DateTime, NaiveDate, offset::Utc};
use diesel::Connection;
use eframe::egui;
use egui::{Context, Frame, Ui};
use egui_async::{Bind, EguiAsyncPlugin};

pub struct AstroCalcApp {
    pub panel_view: usize,
    pub location_map: LocationMap,
    pub lat: f64,
    pub long: f64,
    pub view_windows: Vec<ViewWindow>,
    pub bortle_class: u8,
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

    pub tle_cache: TleCache,
    pub iss_bind: Bind<IssPredictionBundle, String>,
    pub iss_req_key: Option<(NaiveDate, i64, i64, bool)>,
    pub iss_done_key: Option<(NaiveDate, i64, i64, bool)>,

    pub selected_output_tz: String,
    pub selected_output_tz_obj: chrono_tz::Tz,

    pub long_term_data: LongTermPlot,
    pub daily_solar_data: DailySolar,
    pub iss_data: IssPanelState,
    pub database_url: String,
}

impl AstroCalcApp {
    pub fn new(
        egui_ctx: Context,
        settings: AppSettings,
        database_url: String,
        weather_cache: WeatherCache,
        tle_cache: TleCache,
    ) -> Self {
        let lat = settings.lat;
        let long = settings.lon;
        let site_tz = site_tz_from_lat_lon(lat, long);
        let mut app = Self {
            panel_view: 0,
            location_map: LocationMap::new(egui_ctx, long, lat),
            lat,
            long,
            view_windows: settings.view_windows.clone(),
            bortle_class: settings.bortle_class,
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
            tle_cache,
            iss_bind: Bind::new(true),
            iss_req_key: None,
            iss_done_key: None,
            selected_output_tz: site_tz.name().to_string(),
            selected_output_tz_obj: site_tz,
            long_term_data: LongTermPlot::new(LatLon::new(lat, long), database_url.clone()),
            daily_solar_data: DailySolar::new(
                Utc::now().date_naive(),
                LatLon::new(lat, long),
                vec![],
                database_url.clone(),
            ),
            iss_data: IssPanelState::new(
                LatLon::new(lat, long),
                settings.view_windows,
                settings.bortle_class,
            ),
            database_url,
        };
        app.daily_solar_data.set_local_tz(site_tz);
        app.iss_data.set_local_tz(site_tz);
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
            ui.selectable_value(&mut self.panel_view, 3, "ISS");
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
        if self.panel_view == 3 && prev != 3 {
            self.iss_data.lat_lon = LatLon::new(self.lat, self.long);
            self.iss_data.view_windows = self.view_windows.clone();
            self.iss_data.set_local_tz(self.selected_output_tz_obj);
            if self
                .iss_data
                .reload_cached_only(&self.database_url, &self.tle_cache)
            {
                self.iss_done_key = Some(self.iss_key(false));
                self.iss_data.request_predict = false;
            } else {
                self.iss_data.request_predict = true;
            }
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
            bortle_class: &mut self.bortle_class,
            zone_editor: &mut self.zone_editor,
            location_map: &mut self.location_map,
        }
        .show(ui);
        self.iss_data.bortle_class = self.bortle_class;
        if changed {
            self.sync_site_timezone_from_map();
            self.daily_solar_data.lat_lon = LatLon::new(self.lat, self.long);
            self.long_term_data.lat_lon = LatLon::new(self.lat, self.long);
            self.iss_data.lat_lon = LatLon::new(self.lat, self.long);
            self.iss_data.view_windows = self.view_windows.clone();
            self.ephemeris_req_key = None;
            self.ephemeris_done_key = None;
            self.long_term_req_key = None;
            self.long_term_done_key = None;
            self.long_term_dso_batch_key = None;
            self.long_term_dso_done_key = None;
            self.iss_req_key = None;
            self.iss_done_key = None;
            self.iss_data.request_predict = true;
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
        self.iss_data.set_local_tz(tz);
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
            ensure_dso_batch(&db, lat, lon, SAMPLE_FREQ_MINUTES, &ids, &batch);
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
        if self.panel_view == 3 {
            return Utc::now();
        }
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
        let (lat, lon, date) = if self.panel_view == 3 {
            (
                self.iss_data.lat_lon.lat,
                self.iss_data.lat_lon.lon,
                Utc::now().date_naive(),
            )
        } else {
            (
                self.daily_solar_data.lat_lon.lat,
                self.daily_solar_data.lat_lon.lon,
                self.daily_solar_data.date,
            )
        };
        let snapped = self.weather_cache.snap_location(Location { lat, lon });
        (
            date,
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
        let (lat, lon) = if self.panel_view == 3 {
            (self.iss_data.lat_lon.lat, self.iss_data.lat_lon.lon)
        } else {
            (
                self.daily_solar_data.lat_lon.lat,
                self.daily_solar_data.lat_lon.lon,
            )
        };
        let req = WeatherRequest {
            location: Location { lat, lon },
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

    fn sync_weather_into_iss(&mut self) {
        let pending = self.weather_bind.is_pending();
        self.iss_data.weather_pending = pending;
        if pending {
            return;
        }
        match self.weather_bind.read() {
            Some(Ok(snap)) => {
                self.iss_data.weather_snapshot = Some(snap.clone());
            }
            Some(Err(_)) | None => {}
        }
    }

    fn iss_key(&self, force: bool) -> (NaiveDate, i64, i64, bool) {
        // Full-precision site key (0.0001° ~ 10 m) — not the 0.01° ephemeris sector.
        (
            self.iss_data.start_date,
            (self.lat * 10_000.0).round() as i64,
            (self.long * 10_000.0).round() as i64,
            force,
        )
    }

    fn apply_iss_bundle(&mut self, bundle: IssPredictionBundle) {
        self.iss_data.apply_bundle(
            bundle.passes.clone(),
            bundle.sun_transits.clone(),
            bundle.moon_transits.clone(),
            bundle.tle.tle_epoch,
            bundle.tle.fetched_at,
        );

        let lat = self.lat;
        let lon = self.long;
        let db = self.database_url.clone();
        if let Ok(rows) = IssEventInsert::from_bundle(&bundle, lat, lon) {
            if let Ok(mut conn) = diesel::SqliteConnection::establish(&db) {
                let _ = IssEventRow::replace_for_site(&mut conn, lat, lon, &rows);
            }
        }
    }

    fn ensure_iss_predict(&mut self) {
        let force = self.iss_data.force_refresh;
        let key = self.iss_key(force);
        let pending = self.iss_bind.is_pending();
        self.iss_data.pending = pending;

        if pending {
            return;
        }

        if self.iss_req_key == Some(key) && self.iss_done_key != Some(key) {
            let outcome: Option<Result<IssPredictionBundle, String>> = match self.iss_bind.read() {
                Some(Ok(bundle)) => Some(Ok(bundle.clone())),
                Some(Err(e)) => Some(Err(e.clone())),
                None => None,
            };
            if let Some(res) = outcome {
                match res {
                    Ok(bundle) => {
                        self.iss_done_key = Some(key);
                        self.apply_iss_bundle(bundle);
                    }
                    Err(e) => {
                        self.iss_data.error = Some(e);
                        self.iss_data.request_predict = false;
                        self.iss_data.force_refresh = false;
                    }
                }
                return;
            }
        }

        if !self.iss_data.request_predict && !force {
            return;
        }

        if !force
            && self.iss_done_key.as_ref().map(|k| (k.0, k.1, k.2)) == Some((key.0, key.1, key.2))
        {
            self.iss_data.request_predict = false;
            return;
        }

        self.iss_req_key = Some(key);
        let cache = self.tle_cache.clone();
        let lat = self.lat;
        let lon = self.long;
        let date = self.iss_data.start_date;
        let windows = self.view_windows.clone();
        let force_fetch = force;
        self.iss_data.force_refresh = false;
        self.iss_data.pending = true;
        self.iss_bind.refresh(async move {
            fetch_and_predict(
                &cache,
                force_fetch,
                lat,
                lon,
                date,
                ISS_PREDICT_DAY_COUNT,
                &windows,
                true,
            )
        });
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
                        self.daily_solar_data
                            .set_local_tz(self.selected_output_tz_obj);
                        self.daily_solar_data.view_windows = self.view_windows.clone();
                        self.daily_solar_data.refresh_positions();
                    }
                }
                if self.panel_view == 2 {
                    self.daily_solar_data.lat_lon = LatLon::new(self.lat, self.long);
                    self.daily_solar_data
                        .set_local_tz(self.selected_output_tz_obj);
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
                if self.panel_view == 3 {
                    self.iss_data.lat_lon = LatLon::new(self.lat, self.long);
                    self.iss_data.view_windows = self.view_windows.clone();
                    self.iss_data.bortle_class = self.bortle_class;
                    self.iss_data.set_local_tz(self.selected_output_tz_obj);
                    self.refresh_weather_if_needed();
                    self.sync_weather_into_iss();
                    self.ensure_iss_predict();
                    ui.add(&mut self.iss_data);
                }
            });
    }
}
