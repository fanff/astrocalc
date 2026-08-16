use crate::deepsky::ensure_dso_positions;
use crate::models::ObjectPositionStored;
use crate::solarsystemcalc::NightInfo;
use crate::weather_cache::WeatherSnapshot;
use crate::{
    config::ViewWindow,
    models::DateInfo,
    solarsystemcalc::{ObjectPosition, ObjectPositionSegments},
    widgets::{
        CatalogSelection, calendar_plot::CalPlot, sky_map::SkyMapPlot, weather::WeatherPanel,
    },
};
use chrono::NaiveDate;
use chrono_tz::Tz;
use diesel::{Connection, SqliteConnection};
use egui::{Response, RichText};
use egui_extras::{DatePickerButton, Size, StripBuilder};
use std::collections::HashMap;

use crate::panels::LatLon;

/// Nights to precompute ahead of the selected Daily date (inclusive window length).
pub const DAILY_PREFETCH_DAY_COUNT: i64 = 10;
pub const SAMPLE_FREQ_MINUTES: i64 = 10;

const SEGMENT_GAP_MINUTES: i64 = 10;
const SEGMENT_MIN_DURATION_MINUTES: i64 = 60;

#[derive(Clone, PartialEq)]
struct SegmentCacheKey {
    date: NaiveDate,
    lat_sector: f64,
    lon_sector: f64,
    view_windows: Vec<ViewWindow>,
    selected_names: Vec<String>,
    /// Fingerprint of loaded positions (len + first/last sample times).
    pos_fp: Option<(usize, i64, i64)>,
}

pub struct DailySolar {
    pub date: NaiveDate,
    pub lat_lon: LatLon,
    pub catalog_select: CatalogSelection,
    pub sky_map: SkyMapPlot,
    pub cal_plot: CalPlot,
    pub view_windows: Vec<ViewWindow>,
    pub positions: Option<Vec<ObjectPosition>>,
    pub dateinfo: Option<NightInfo>,
    pub database_connection: String,
    /// Filled by the app shell from weather Bind (Daily does not fetch).
    pub weather_snapshot: Option<WeatherSnapshot>,
    pub weather_pending: bool,
    pub weather_error: Option<String>,
    /// Observer local timezone (from map lat/lon).
    pub local_tz: Tz,
    /// Set by Weather Refresh button; app shell clears and forces a fetch.
    pub weather_force_refresh: bool,
    /// Ask app shell to run background ephemeris prefetch (selected day + window).
    pub request_ephemeris_prefetch: bool,
    /// True while app-shell Bind is computing solar positions.
    pub ephemeris_pending: bool,
    /// Cached DSO type labels from last deep-sky load.
    dso_types: HashMap<String, String>,
    /// Weather section expanded (folds vertically).
    pub weather_open: bool,
    /// Radar / sky map expanded (folds from the left).
    pub radar_open: bool,
    /// Filtered segments rebuilt only when inputs change.
    cached_segments: ObjectPositionSegments,
    segment_cache_key: Option<SegmentCacheKey>,
    /// Object type map for calendar plot (rebuilt with segments).
    cached_object_types: HashMap<String, String>,
}
impl DailySolar {
    pub fn new(
        date: NaiveDate,
        lat_lon: LatLon,
        view_windows: Vec<ViewWindow>,
        database_connection_str: String,
    ) -> Self {
        Self {
            date,
            lat_lon,
            catalog_select: CatalogSelection::default(),
            sky_map: SkyMapPlot::new(),
            cal_plot: CalPlot::new(),
            view_windows,
            positions: None,
            dateinfo: None,
            database_connection: database_connection_str,
            weather_snapshot: None,
            weather_pending: false,
            weather_error: None,
            local_tz: Tz::UTC,
            weather_force_refresh: false,
            request_ephemeris_prefetch: true,
            ephemeris_pending: false,
            dso_types: HashMap::new(),
            weather_open: true,
            radar_open: true,
            cached_segments: ObjectPositionSegments::new(),
            segment_cache_key: None,
            cached_object_types: CatalogSelection::planet_type_map(),
        }
    }

    pub fn set_local_tz(&mut self, tz: Tz) {
        self.local_tz = tz;
        self.cal_plot.output_timezone = tz;
        self.sky_map.local_tz = tz;
    }

    /// Load cached solar (+ DSO) for the selected day. Does not compute solar ephemeris.
    /// When `request_prefetch` is true, ask the app shell to fill selected day + 10 nights.
    pub fn refresh_positions(&mut self) {
        self.refresh_positions_inner(true);
    }

    pub fn reload_cached_only(&mut self) {
        self.refresh_positions_inner(false);
    }

    fn refresh_positions_inner(&mut self, request_prefetch: bool) {
        let snapped_lat_lon = self.lat_lon.snap(2);
        let mut conn: SqliteConnection =
            SqliteConnection::establish(&self.database_connection).unwrap();

        match DateInfo::from_db(&mut conn, self.date, &snapped_lat_lon) {
            Some(date_info) => {
                self.dateinfo = Some(date_info.as_nightinfo());
                let positions =
                    ObjectPositionStored::read_from_db(&mut conn, self.date, snapped_lat_lon);
                self.positions = if positions.is_empty() {
                    None
                } else {
                    Some(positions)
                };
                self.refresh_dso_positions(&mut conn);
            }
            None => {
                self.dateinfo = None;
                self.positions = None;
                self.dso_types.clear();
            }
        }
        self.segment_cache_key = None;
        if request_prefetch {
            self.request_ephemeris_prefetch = true;
        }
    }

    fn refresh_dso_positions(&mut self, conn: &mut SqliteConnection) {
        self.dso_types.clear();
        let Some(night) = self.dateinfo.as_ref() else {
            return;
        };
        let ids = self.catalog_select.selected_dso_ids();
        if ids.is_empty() {
            return;
        }
        let snapped = self.lat_lon.snap(2);
        let (dso_pos, types) = ensure_dso_positions(
            conn,
            night,
            snapped.lat,
            snapped.lon,
            SAMPLE_FREQ_MINUTES,
            &ids,
        );
        self.dso_types = types;
        if dso_pos.is_empty() {
            return;
        }
        match &mut self.positions {
            Some(existing) => existing.extend(dso_pos),
            None => self.positions = Some(dso_pos),
        }
    }

    fn positions_fingerprint(positions: Option<&Vec<ObjectPosition>>) -> Option<(usize, i64, i64)> {
        let pos = positions?;
        if pos.is_empty() {
            return Some((0, 0, 0));
        }
        Some((
            pos.len(),
            pos.first()?.utc_datetime.timestamp_millis(),
            pos.last()?.utc_datetime.timestamp_millis(),
        ))
    }

    /// Rebuild filtered segments when inputs change. Returns true if rebuilt.
    fn ensure_segment_cache(&mut self) -> bool {
        let snapped = self.lat_lon.snap(2);
        let selected_names = self.catalog_select.selected_object_names();
        let key = SegmentCacheKey {
            date: self.date,
            lat_sector: snapped.lat,
            lon_sector: snapped.lon,
            view_windows: self.view_windows.clone(),
            selected_names: selected_names.clone(),
            pos_fp: Self::positions_fingerprint(self.positions.as_ref()),
        };
        if self.segment_cache_key.as_ref() == Some(&key) {
            return false;
        }

        if let Some(object_pos) = &self.positions {
            self.cached_segments =
                ObjectPositionSegments::from_positions(object_pos, SEGMENT_GAP_MINUTES)
                    .filter_view(
                        &self.view_windows,
                        SEGMENT_MIN_DURATION_MINUTES,
                        &selected_names,
                    );
            let mut types = CatalogSelection::planet_type_map();
            types.extend(self.dso_types.clone());
            self.cached_object_types = types;
        } else {
            self.cached_segments = ObjectPositionSegments::new();
            self.cached_object_types = CatalogSelection::planet_type_map();
        }
        self.segment_cache_key = Some(key);
        true
    }
}
impl egui::Widget for &mut DailySolar {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(280.0, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if ui.button("<<").clicked() {
                        self.date = self.date.pred_opt().unwrap();
                        self.refresh_positions();
                    }
                    let date_pick = DatePickerButton::new(&mut self.date);
                    if ui.add(date_pick).changed() {
                        self.refresh_positions();
                    }
                    if ui.button(">>").clicked() {
                        self.date = self.date.succ_opt().unwrap();
                        self.refresh_positions();
                    }
                },
            );
        });

        if self.ephemeris_pending {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Caching ephemeris (selected night + 10 days)…");
            });
        } else if self.dateinfo.is_none() {
            ui.label("No cached data for this night yet — calculating in background…");
        }

        let weather_label = if self.weather_open {
            "▼  Weather"
        } else {
            "▶  Weather"
        };
        if ui
            .add(egui::Button::new(RichText::new(weather_label).strong()).frame(true))
            .clicked()
        {
            self.weather_open = !self.weather_open;
        }
        if self.weather_open {
            ui.add(WeatherPanel {
                snapshot: self.weather_snapshot.as_ref(),
                pending: self.weather_pending,
                error: self.weather_error.as_deref(),
                night: self.dateinfo.as_ref(),
                local_tz: self.local_tz,
                force_refresh: &mut self.weather_force_refresh,
            });
        }

        let dso_before = self.catalog_select.selected_dso_ids();
        ui.add(&mut self.catalog_select);
        let dso_after = self.catalog_select.selected_dso_ids();
        if dso_before != dso_after {
            self.refresh_positions();
        }

        let segments_rebuilt = self.ensure_segment_cache();
        self.cal_plot.dateinfo = self.dateinfo.clone();
        if segments_rebuilt {
            self.cal_plot.positions_map = self.cached_segments.clone();
            self.cal_plot.object_types = self.cached_object_types.clone();
            self.sky_map.op_segs = self.cached_segments.clone();
        }

        let plots_h = ui.available_height().max(160.0);
        let plots_w = ui.available_width();
        let fold_btn_w = 28.0;
        let sky_side = if self.radar_open {
            plots_h
                .min((plots_w - fold_btn_w) * 0.45)
                .clamp(140.0, 520.0)
        } else {
            0.0
        };

        ui.allocate_ui_with_layout(
            egui::vec2(plots_w, plots_h),
            egui::Layout::left_to_right(egui::Align::TOP),
            |ui| {
                let radar_btn = if self.radar_open {
                    "◀\nR\na\nd\na\nr"
                } else {
                    "▶\nR\na\nd\na\nr"
                };
                let btn_h = plots_h.min(ui.available_height()).max(120.0);
                if ui
                    .add_sized(
                        egui::vec2(fold_btn_w, btn_h),
                        egui::Button::new(RichText::new(radar_btn).small()).frame(true),
                    )
                    .on_hover_text(if self.radar_open {
                        "Hide radar view"
                    } else {
                        "Show radar view"
                    })
                    .clicked()
                {
                    self.radar_open = !self.radar_open;
                }

                if self.radar_open {
                    StripBuilder::new(ui)
                        .size(Size::exact(sky_side))
                        .size(Size::remainder().at_least(160.0))
                        .horizontal(|mut strip| {
                            strip.cell(|ui| {
                                ui.set_min_height(ui.available_height());
                                ui.set_max_height(ui.available_height());
                                ui.add(&mut self.sky_map);
                            });
                            strip.cell(|ui| {
                                ui.set_min_height(ui.available_height());
                                ui.add(&mut self.cal_plot);
                            });
                        });
                } else {
                    ui.set_min_height(plots_h);
                    ui.add(&mut self.cal_plot);
                }
            },
        );

        ui.response()
    }
}

pub fn is_in_viewwindow(pos: &ObjectPosition, view_windows: &Vec<ViewWindow>) -> bool {
    let az = pos.azimuth;
    let alt = pos.altitude;
    for vw in view_windows {
        if vw.contains(az, alt) {
            return true;
        }
    }
    false
}
