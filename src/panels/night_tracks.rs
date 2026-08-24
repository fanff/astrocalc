use chrono::{Datelike, NaiveDate, Utc};
use chrono_tz::Tz;
use diesel::{Connection, SqliteConnection};
use egui::Response;

use crate::config::ViewWindow;
use crate::deepsky::{cached_dso_positions, nights_needing_selected_dso};
use crate::models::{DateInfo, ObjectPositionStored};
use crate::panels::LatLon;
use crate::panels::dailysolar::DAILY_PREFETCH_DAY_COUNT;
use crate::solarsystemcalc::ObjectPositionSegments;
use crate::widgets::catalog_select::CatalogSelection;
use crate::widgets::night_timeline_plot::{NightTimelinePlot, NightTimelineRow};

const SEGMENT_GAP_MINUTES: i64 = 10;
const MIN_DURATION_MINUTES: i64 = 0;

fn date_to_y(date: NaiveDate) -> f64 {
    date.num_days_from_ce() as f64
}

#[derive(Clone, PartialEq)]
struct TimelineRefreshKey {
    lat_sector: f64,
    lon_sector: f64,
    view_windows: Vec<ViewWindow>,
    apply_view_filter: bool,
    selected_names: Vec<String>,
    dates: Vec<NaiveDate>,
}

pub struct NightTracksPanel {
    pub catalog_select: CatalogSelection,
    pub view_windows: Vec<ViewWindow>,
    /// When true, clip segments to configured visibility zones.
    pub apply_view_filter: bool,
    pub lat_lon: LatLon,
    pub local_tz: Tz,
    database_url: String,
    plot: NightTimelinePlot,
    pub last_cached_date: Option<NaiveDate>,
    pub prefetch_start: Option<NaiveDate>,
    last_prefetch_requested: Option<NaiveDate>,
    pub ephemeris_pending: bool,
    pub goto_daily_date: Option<NaiveDate>,
    pub dso_backfill_queue: Vec<NaiveDate>,
    pub dso_backfill_pending: bool,
    refresh_cache_key: Option<TimelineRefreshKey>,
}

impl NightTracksPanel {
    pub fn new(lat_lon: LatLon, database_url: String) -> Self {
        Self {
            catalog_select: CatalogSelection::default(),
            view_windows: Vec::new(),
            apply_view_filter: true,
            lat_lon,
            local_tz: Tz::UTC,
            database_url,
            plot: NightTimelinePlot {
                rows: Vec::new(),
                local_tz: Tz::UTC,
                lat_deg: lat_lon.lat,
                lon_deg: lat_lon.lon,
                reset_view: true,
                clicked_date: None,
                view_max_y: f64::NAN,
            },
            last_cached_date: None,
            prefetch_start: None,
            last_prefetch_requested: None,
            ephemeris_pending: false,
            goto_daily_date: None,
            dso_backfill_queue: Vec::new(),
            dso_backfill_pending: false,
            refresh_cache_key: None,
        }
    }

    pub fn refresh_from_db(&mut self) {
        self.refresh_from_db_inner(true);
    }

    pub fn reload_after_prefetch(&mut self) {
        self.refresh_cache_key = None;
        self.refresh_from_db_inner(false);
        self.prefetch_start = None;
    }

    pub fn reload_after_dso_backfill(&mut self) {
        self.refresh_cache_key = None;
        self.refresh_from_db_inner(false);
    }

    pub fn take_dso_backfill_batch(&mut self, batch: usize) -> Vec<NaiveDate> {
        let n = batch.min(self.dso_backfill_queue.len());
        self.dso_backfill_queue.drain(..n).collect()
    }

    fn refresh_from_db_inner(&mut self, reset_view: bool) {
        let snapped = self.lat_lon.snap(2);
        let mut conn = SqliteConnection::establish(&self.database_url).unwrap_or_else(|_| {
            panic!("Error connecting to {}", self.database_url);
        });

        let mut dates = ObjectPositionStored::available_days(&mut conn, &snapped);
        dates.sort();
        self.last_cached_date = dates.last().copied();

        let selected = self.catalog_select.selected_object_names();

        let key = TimelineRefreshKey {
            lat_sector: snapped.lat,
            lon_sector: snapped.lon,
            view_windows: self.view_windows.clone(),
            apply_view_filter: self.apply_view_filter,
            selected_names: selected.clone(),
            dates: dates.clone(),
        };
        if self.refresh_cache_key.as_ref() == Some(&key) {
            if reset_view {
                self.plot.reset_view = true;
            }
            let dso_ids = self.catalog_select.selected_dso_ids();
            self.dso_backfill_queue =
                nights_needing_selected_dso(&mut conn, snapped.lat, snapped.lon, &dates, &dso_ids);
            self.plot.lat_deg = self.lat_lon.lat;
            self.plot.lon_deg = self.lat_lon.lon;
            self.plot.local_tz = self.local_tz;
            return;
        }

        let dso_ids = self.catalog_select.selected_dso_ids();
        self.dso_backfill_queue =
            nights_needing_selected_dso(&mut conn, snapped.lat, snapped.lon, &dates, &dso_ids);

        let mut rows: Vec<NightTimelineRow> = Vec::new();

        for date in dates {
            let Some(date_info) = DateInfo::from_db(&mut conn, date, &snapped) else {
                continue;
            };
            let night = date_info.as_nightinfo();
            let mut positions = ObjectPositionStored::read_from_db(&mut conn, date, snapped);
            if !dso_ids.is_empty() {
                let dso_pos =
                    cached_dso_positions(&mut conn, date, snapped.lat, snapped.lon, &dso_ids);
                positions.extend(dso_pos);
            }
            let segments = if positions.is_empty() {
                ObjectPositionSegments::new()
            } else {
                ObjectPositionSegments::from_positions(&positions, SEGMENT_GAP_MINUTES).filter_view(
                    &self.view_windows,
                    MIN_DURATION_MINUTES,
                    &selected,
                    self.apply_view_filter,
                )
            };
            rows.push(NightTimelineRow::new(
                date,
                night,
                segments,
                self.lat_lon.lat,
                self.lat_lon.lon,
            ));
        }

        self.plot.rows = rows;
        self.plot.lat_deg = self.lat_lon.lat;
        self.plot.lon_deg = self.lat_lon.lon;
        self.plot.local_tz = self.local_tz;
        self.refresh_cache_key = Some(key);
        if reset_view {
            self.plot.reset_view = true;
            self.last_prefetch_requested = None;
        }

        if self.last_cached_date.is_none() && self.prefetch_start.is_none() {
            let today = Utc::now().date_naive();
            self.prefetch_start = Some(today);
            self.last_prefetch_requested = Some(today);
        }
    }

    pub fn maybe_request_prefetch_from_view(&mut self, view_max_y: f64) {
        if self.ephemeris_pending {
            return;
        }
        let Some(last) = self.last_cached_date else {
            return;
        };
        let last_y = date_to_y(last);
        if view_max_y <= last_y + 0.5 {
            return;
        }
        let Some(next) = last.succ_opt() else {
            return;
        };
        if self.last_prefetch_requested == Some(next) {
            return;
        }
        self.prefetch_start = Some(next);
        self.last_prefetch_requested = Some(next);
    }
}

impl egui::Widget for &mut NightTracksPanel {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let before = self.catalog_select.selected_object_names();
        ui.add(&mut self.catalog_select);
        let after = self.catalog_select.selected_object_names();
        if before != after {
            self.refresh_from_db();
        }

        if ui
            .checkbox(&mut self.apply_view_filter, "Limit to configured view")
            .changed()
        {
            self.refresh_cache_key = None;
            self.refresh_from_db();
        }

        if self.ephemeris_pending {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Caching next {} nights…", DAILY_PREFETCH_DAY_COUNT));
            });
        }
        if self.dso_backfill_pending || !self.dso_backfill_queue.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "Caching deep-sky tracks in background… {} night(s) still queued",
                    self.dso_backfill_queue.len()
                ));
            });
        }

        let plot_response = ui.add(&mut self.plot);
        if let Some(date) = self.plot.clicked_date {
            self.goto_daily_date = Some(date);
        }

        if self.plot.view_max_y.is_finite() {
            self.maybe_request_prefetch_from_view(self.plot.view_max_y);
        }

        plot_response
    }
}
