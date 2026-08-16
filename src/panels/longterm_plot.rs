use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use chrono_tz::Tz;
use diesel::{Connection, SqliteConnection};
use egui::{Align2, Color32, Id, Response, RichText, Sense, Stroke, Vec2, Vec2b};
use egui_plot::{AxisHints, GridInput, GridMark, Plot, PlotBounds, PlotPoint, Points, Text, VLine};
use std::collections::HashMap;

use crate::config::ViewWindow;
use crate::deepsky::{cached_dso_positions, nights_needing_selected_dso};
use crate::models::{DateInfo, ObjectPositionStored};
use crate::panels::LatLon;
use crate::panels::dailysolar::DAILY_PREFETCH_DAY_COUNT;
use crate::solarsystemcalc::{ObjectPositionSegments, get_object_color};
use crate::timezone_util::{format_hm_local, format_hm_utc};
use crate::widgets::CatalogSelection;

const LONG_TERM_MIN_MINUTES: i64 = 20;
/// Default visible span from today when opening / resetting the Long Term plot.
const DEFAULT_VIEW_DAYS: f64 = 30.0;
/// Extra plot space below the first object row so dots/labels at y=0 are not clipped.
const Y_PAD_BOTTOM: f64 = 0.4;
const Y_PAD_TOP: f64 = 0.5;

fn date_to_x(date: NaiveDate) -> f64 {
    date.num_days_from_ce() as f64
}

/// Fractional day position for "now" on the date axis.
fn now_x(now: DateTime<Utc>) -> f64 {
    let day = date_to_x(now.date_naive());
    let frac = now.num_seconds_from_midnight() as f64 / 86_400.0;
    day + frac
}

fn x_to_date(x: f64) -> Option<NaiveDate> {
    NaiveDate::from_num_days_from_ce_opt(x.round() as i32)
}

/// Default visible window: today on the left, 30 days ahead.
fn today_aligned_x_bounds(_nights: &[NightPresence]) -> (f64, f64) {
    let today = chrono::Utc::now().date_naive();
    let today_x = date_to_x(today);
    let xmin = today_x - 0.5;
    let xmax = today_x + DEFAULT_VIEW_DAYS + 0.5;
    (xmin, xmax)
}

/// egui_plot defaults near 0 → year 0000 on our CE day axis; treat that as "needs reset".
fn bounds_look_like_epoch(xmin: f64, xmax: f64) -> bool {
    let today_x = date_to_x(chrono::Utc::now().date_naive());
    xmax < today_x - 365.0 * 50.0 || xmin < 1.0
}

fn series_key(name: &str, date: NaiveDate) -> String {
    format!("{name}@{date}")
}

/// Visibility summary for one object on one night (after view-window filter).
#[derive(Clone, Debug)]
struct ObjectNightTip {
    name: String,
    date: NaiveDate,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    /// Sum of in-view segment durations (minutes).
    duration_minutes: i64,
    /// Best (lowest) magnitude across samples; Moon may be a stub value.
    magnitude: f64,
}

/// Presence of an object on a given night (after view-window filter).
#[derive(Clone, Debug)]
struct NightPresence {
    date: NaiveDate,
    objects: Vec<ObjectNightTip>,
}

pub struct LongTermPlot {
    pub catalog_select: CatalogSelection,
    pub view_windows: Vec<ViewWindow>,
    pub lat_lon: LatLon,
    /// Observer local timezone (for tooltip hours).
    pub local_tz: Tz,
    database_url: String,
    nights: Vec<NightPresence>,
    /// Stable Y-row order for selected names (updated on refresh).
    object_rows: Vec<String>,
    /// When true, next plot frame resets camera to fit data.
    reset_view: bool,
    /// Last night present in the solar cache for this sector (may be empty of visible objects).
    pub last_cached_date: Option<NaiveDate>,
    /// Ask app shell to prefetch 10 nights starting at this date (background Bind).
    pub prefetch_start: Option<NaiveDate>,
    /// Avoid re-requesting the same batch while it is in flight / just requested.
    last_prefetch_requested: Option<NaiveDate>,
    pub ephemeris_pending: bool,
    /// Set when the user clicks a presence dot; app shell opens Daily for this date.
    pub goto_daily_date: Option<NaiveDate>,
    /// Nights still missing selected DSO tracks (filled in background, 10 at a time).
    pub dso_backfill_queue: Vec<NaiveDate>,
    /// True while a DSO backfill Bind is running.
    pub dso_backfill_pending: bool,
    /// Skip full segment rebuild when sector/windows/selection/dates unchanged.
    refresh_cache_key: Option<LtRefreshKey>,
}

#[derive(Clone, PartialEq)]
struct LtRefreshKey {
    lat_sector: f64,
    lon_sector: f64,
    view_windows: Vec<ViewWindow>,
    selected_names: Vec<String>,
    dates: Vec<NaiveDate>,
}

impl LongTermPlot {
    pub fn new(lat_lon: LatLon, database_url: String) -> Self {
        Self {
            catalog_select: CatalogSelection::default(),
            view_windows: Vec::new(),
            lat_lon,
            local_tz: Tz::UTC,
            database_url,
            nights: Vec::new(),
            object_rows: Vec::new(),
            reset_view: true,
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

    /// Reload after a background prefetch without resetting the camera.
    pub fn reload_after_prefetch(&mut self) {
        self.refresh_cache_key = None;
        self.refresh_from_db_inner(false);
        self.prefetch_start = None;
    }

    /// Reload after a DSO backfill batch (keeps camera; may leave more work in the queue).
    pub fn reload_after_dso_backfill(&mut self) {
        self.refresh_cache_key = None;
        self.refresh_from_db_inner(false);
    }

    /// Take up to `batch` nights from the front of the DSO backfill queue.
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
        self.object_rows = selected.clone();

        let key = LtRefreshKey {
            lat_sector: snapped.lat,
            lon_sector: snapped.lon,
            view_windows: self.view_windows.clone(),
            selected_names: selected.clone(),
            dates: dates.clone(),
        };
        if self.refresh_cache_key.as_ref() == Some(&key) {
            if reset_view {
                self.reset_view = true;
            }
            let dso_ids = self.catalog_select.selected_dso_ids();
            self.dso_backfill_queue =
                nights_needing_selected_dso(&mut conn, snapped.lat, snapped.lon, &dates, &dso_ids);
            return;
        }

        let dso_ids = self.catalog_select.selected_dso_ids();
        // Queue nights that still need DSO computation (non-blocking; Bind fills them).
        self.dso_backfill_queue =
            nights_needing_selected_dso(&mut conn, snapped.lat, snapped.lon, &dates, &dso_ids);

        let mut nights = Vec::new();

        for date in dates {
            let Some(date_info) = DateInfo::from_db(&mut conn, date, &snapped) else {
                continue;
            };
            let _night = date_info.as_nightinfo();
            let mut positions = ObjectPositionStored::read_from_db(&mut conn, date, snapped);
            if !dso_ids.is_empty() {
                // Cached only — never compute on the UI thread.
                let dso_pos =
                    cached_dso_positions(&mut conn, date, snapped.lat, snapped.lon, &dso_ids);
                positions.extend(dso_pos);
            }
            if positions.is_empty() {
                continue;
            }
            let filtered = ObjectPositionSegments::from_positions(&positions, 10).filter_view(
                &self.view_windows,
                LONG_TERM_MIN_MINUTES,
                &selected,
            );
            let mut objects: Vec<ObjectNightTip> = filtered
                .segments
                .iter()
                .filter_map(|(name, segs)| tip_from_segments(name, date, segs))
                .collect();
            objects.sort_by(|a, b| a.name.cmp(&b.name));
            if !objects.is_empty() {
                nights.push(NightPresence { date, objects });
            }
        }

        self.nights = nights;
        self.refresh_cache_key = Some(key);
        if reset_view {
            self.reset_view = true;
            self.last_prefetch_requested = None;
        }

        // Empty cache: kick off an initial 10-day batch from today.
        if self.last_cached_date.is_none() && self.prefetch_start.is_none() {
            let today = chrono::Utc::now().date_naive();
            self.prefetch_start = Some(today);
            self.last_prefetch_requested = Some(today);
        }
    }

    fn maybe_request_prefetch_from_view(&mut self, view_max_x: f64) {
        if self.ephemeris_pending {
            return;
        }
        let Some(last) = self.last_cached_date else {
            return;
        };
        let last_x = date_to_x(last);
        // When the right edge moves past the last cached night, fetch the next batch.
        if view_max_x <= last_x + 0.5 {
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

fn tip_from_segments(
    name: &str,
    date: NaiveDate,
    segs: &[crate::solarsystemcalc::ObjectSegment],
) -> Option<ObjectNightTip> {
    let mut start: Option<DateTime<Utc>> = None;
    let mut end: Option<DateTime<Utc>> = None;
    let mut duration_minutes: i64 = 0;
    let mut magnitude = f64::INFINITY;

    for seg in segs {
        let first = seg.iter().next()?;
        let last = seg.iter().last()?;
        duration_minutes += last
            .utc_datetime
            .signed_duration_since(first.utc_datetime)
            .num_minutes();
        start = Some(match start {
            Some(s) => s.min(first.utc_datetime),
            None => first.utc_datetime,
        });
        end = Some(match end {
            Some(e) => e.max(last.utc_datetime),
            None => last.utc_datetime,
        });
        for p in seg.iter() {
            magnitude = magnitude.min(p.magnitude);
        }
    }

    let start = start?;
    let end = end?;
    if !magnitude.is_finite() {
        magnitude = 99.0;
    }

    Some(ObjectNightTip {
        name: name.to_string(),
        date,
        start,
        end,
        duration_minutes,
        magnitude,
    })
}

fn format_duration(minutes: i64) -> String {
    let h = minutes / 60;
    let m = minutes % 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m}m")
    }
}

fn format_tip(tip: &ObjectNightTip, tz: Tz) -> String {
    let mag = if tip.magnitude < 90.0 {
        format!("{:.1}", tip.magnitude)
    } else {
        "—".into()
    };
    format!(
        "{name}\n{date}\nVisible: {dur}\nStart: {s_loc} ({s_utc} UTC)\nEnd:   {e_loc} ({e_utc} UTC)\nMag: {mag}\nClick → Daily",
        name = tip.name,
        date = tip.date.format("%Y-%m-%d"),
        dur = format_duration(tip.duration_minutes),
        s_loc = format_hm_local(tip.start, tz),
        s_utc = format_hm_utc(tip.start),
        e_loc = format_hm_local(tip.end, tz),
        e_utc = format_hm_utc(tip.end),
        mag = mag,
    )
}

impl egui::Widget for &mut LongTermPlot {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let before = self.catalog_select.selected_object_names();
        ui.add(&mut self.catalog_select);
        let after = self.catalog_select.selected_object_names();
        if before != after {
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
        if !self.ephemeris_pending
            && self.dso_backfill_queue.is_empty()
            && !self.dso_backfill_pending
            && self.nights.is_empty()
        {
            ui.label(
                "No cached nights yet — calculating in the background, or pan right to extend the range.",
            );
        }

        let row_count = self.object_rows.len().max(1) as f64;
        let y_min_allowed = -Y_PAD_BOTTOM;
        let y_max_allowed = (row_count - 1.0).max(0.0) + Y_PAD_TOP;

        let tips_by_key: HashMap<String, ObjectNightTip> = self
            .nights
            .iter()
            .flat_map(|n| n.objects.iter())
            .map(|t| (series_key(&t.name, t.date), t.clone()))
            .collect();
        let date_by_id: HashMap<Id, NaiveDate> = tips_by_key
            .iter()
            .map(|(k, t)| (Id::new(k), t.date))
            .collect();

        let local_tz = self.local_tz;
        let tips_for_fmt = tips_by_key.clone();
        let date_formatter = |mark: GridMark, _range: &std::ops::RangeInclusive<f64>| {
            x_to_date(mark.value)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        };
        let label_fmt = move |name: &str, _value: &PlotPoint| {
            tips_for_fmt
                .get(name)
                .map(|t| format_tip(t, local_tz))
                .unwrap_or_else(|| name.to_string())
        };

        let x_grid = |input: GridInput| {
            let mut marks = Vec::new();
            let start = input.bounds.0.floor() as i32;
            let end = input.bounds.1.ceil() as i32;
            let span = (end - start).max(1);
            let step = if span > 60 {
                7
            } else if span > 20 {
                2
            } else {
                1
            };
            let mut day = start;
            while day <= end {
                marks.push(GridMark {
                    value: day as f64,
                    step_size: step as f64,
                });
                day += step;
            }
            marks
        };

        // Mouse wheel is Y-only in egui; egui_plot's allow_scroll pans, and with Y disabled
        // that made the wheel a no-op. We map wheel → horizontal zoom ourselves.
        let plot = Plot::new("long_term_presence")
            .height(ui.available_height().max(220.0))
            .width(ui.available_width())
            .allow_zoom(false)
            .allow_drag(true)
            .allow_scroll(false)
            .allow_axis_zoom_drag(Vec2b::new(true, false))
            .show_axes(Vec2b::new(true, false))
            .show_grid(true)
            .show_y(false)
            .sense(Sense::click_and_drag())
            .custom_x_axes(vec![
                AxisHints::new_x().label("Date").formatter(date_formatter),
            ])
            .x_grid_spacer(x_grid)
            .label_formatter(label_fmt);

        let nights = self.nights.clone();
        let object_rows = self.object_rows.clone();
        let reset_view = self.reset_view;
        if reset_view {
            self.reset_view = false;
        }

        let mut view_max_x = f64::NAN;

        let plot_response = plot.show(ui, |plot_ui| {
            let bounds = plot_ui.plot_bounds();
            let mut xmin = bounds.min()[0];
            let mut xmax = bounds.max()[0];
            let need_x_reset = reset_view || bounds_look_like_epoch(xmin, xmax);
            if need_x_reset {
                let (x0, x1) = today_aligned_x_bounds(&nights);
                xmin = x0;
                xmax = x1;
            }

            // Clamp Y: allow a little room below row 0 so dots/labels are not clipped.
            let mut ymin = bounds.min()[1];
            let mut ymax = bounds.max()[1];
            let height = (ymax - ymin).max(0.5);
            if need_x_reset {
                ymin = y_min_allowed;
                ymax = y_max_allowed;
            } else {
                if ymin < y_min_allowed {
                    ymin = y_min_allowed;
                    ymax = ymin + height;
                }
                if ymax > y_max_allowed {
                    ymax = y_max_allowed;
                    ymin = (ymax - height).max(y_min_allowed);
                }
            }
            plot_ui.set_plot_bounds(PlotBounds::from_min_max([xmin, ymin], [xmax, ymax]));

            // Wheel → zoom X around pointer (scroll up = zoom in).
            if plot_ui.response().contains_pointer() {
                let scroll_y = plot_ui.ctx().input(|i| i.smooth_scroll_delta.y);
                if scroll_y.abs() > f32::EPSILON {
                    let zoom_x = (0.01 * scroll_y).exp();
                    plot_ui.zoom_bounds_around_hovered(Vec2::new(zoom_x, 1.0));
                }
            }

            view_max_x = plot_ui.plot_bounds().max()[0].max(xmax);

            let now = chrono::Utc::now();
            let nx = now_x(now);
            plot_ui.vline(
                VLine::new("now", nx)
                    .stroke(Stroke::new(1.5, Color32::from_rgb(220, 80, 80)))
                    .name("now"),
            );
            plot_ui.text(
                Text::new(
                    "now_label",
                    PlotPoint::new(nx, ymax - 0.15),
                    RichText::new("now").size(11.0).strong(),
                )
                .color(Color32::from_rgb(220, 80, 80))
                .anchor(Align2::CENTER_TOP),
            );

            if nights.is_empty() || object_rows.is_empty() {
                return;
            }

            for night in &nights {
                let x = date_to_x(night.date);
                for tip in &night.objects {
                    let Some(yi) = object_rows.iter().position(|n| n == &tip.name) else {
                        continue;
                    };
                    let y = yi as f64;
                    let color = get_object_color(&tip.name);
                    let key = series_key(&tip.name, tip.date);
                    let pts = egui_plot::PlotPoints::from_iter([[x, y]]);
                    plot_ui.points(Points::new(key.clone(), pts).radius(6.0).color(color));
                    // Non-interactive label (unique id so it does not steal tip/click from the dot).
                    plot_ui.text(
                        Text::new(
                            format!("label_{key}"),
                            PlotPoint::new(x, y),
                            RichText::new(tip.name.clone()).size(11.0).strong(),
                        )
                        .color(Color32::WHITE)
                        .anchor(Align2::LEFT_CENTER),
                    );
                }
            }
        });

        if plot_response.response.clicked() {
            if let Some(item_id) = plot_response.hovered_plot_item {
                if let Some(date) = date_by_id.get(&item_id) {
                    self.goto_daily_date = Some(*date);
                }
            }
        }

        if view_max_x.is_finite() {
            self.maybe_request_prefetch_from_view(view_max_x);
        }

        ui.response()
    }
}
