use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use chrono_tz::Tz;
use egui::{Color32, Id, Response, RichText, Sense, Stroke, Vec2, Vec2b};
use egui_plot::{AxisHints, Bar, BarChart, GridInput, GridMark, Plot, PlotBounds, PlotPoint};
use std::collections::HashMap;

use crate::satellites::{Observer, moon_illum_pct, moon_phase_label, observer_moon_altitude_deg};
use crate::solarsystemcalc::{NightInfo, ObjectPositionSegments, get_object_color};
use crate::timezone_util::format_hm_local;

const SAMPLE_STEP_MINUTES: i64 = 10;
const DEFAULT_VIEW_DAYS: f64 = 30.0;
/// Closest zoom (fewest days visible).
const MIN_VIEW_DAYS: f64 = 10.0;
/// Furthest zoom (most days visible).
const MAX_VIEW_DAYS: f64 = 90.0;
const ROW_HEIGHT: f64 = 1.0;
const MS_PER_HOUR: f64 = 3_600_000.0;
/// Fallback night half-width (hours) when no rows intersect the Y span.
const FALLBACK_NIGHT_HALF_HOURS: f64 = 8.0;
/// Illumination below this is treated as "no moon" for row background.
const MOON_ILLUM_MIN_PCT: f64 = 5.0;

const NIGHT_BG_DARK: Color32 = Color32::from_rgb(6, 6, 10);
const NIGHT_BG_MOONLIT: Color32 = Color32::from_rgb(18, 36, 72);
/// Object bar thickness as a fraction of the night row height.
const OBJECT_BAR_HEIGHT: f64 = 0.16;

#[derive(Clone)]
pub struct NightTimelineRow {
    pub date: chrono::NaiveDate,
    pub night: NightInfo,
    pub segments: ObjectPositionSegments,
    /// Cached at load: moon above horizon near night midpoint with meaningful illumination.
    pub moon_present: bool,
    /// Illuminated fraction at night midpoint (`[0, 100]`).
    pub moon_illum_pct: f64,
}

impl NightTimelineRow {
    pub fn new(
        date: chrono::NaiveDate,
        night: NightInfo,
        segments: ObjectPositionSegments,
        lat_deg: f64,
        lon_deg: f64,
    ) -> Self {
        let (moon_present, moon_illum_pct) = night_moon_state(lat_deg, lon_deg, &night);
        Self {
            date,
            night,
            segments,
            moon_present,
            moon_illum_pct,
        }
    }

    pub fn night_quality_label(&self) -> String {
        if !self.moon_present {
            "Dark sky (no moon)".into()
        } else {
            format!("Moonlit — {}", moon_phase_label(self.moon_illum_pct))
        }
    }
}

fn night_midpoint(night: &NightInfo) -> DateTime<Utc> {
    let start = night.night_start_ms.timestamp_millis();
    let end = night.night_end_ms.timestamp_millis();
    DateTime::from_timestamp_millis(start + (end - start) / 2).unwrap_or(night.night_start_ms)
}

/// Moon presence + illumination at mid-night for background / quality.
fn night_moon_state(lat_deg: f64, lon_deg: f64, night: &NightInfo) -> (bool, f64) {
    let mid = night_midpoint(night);
    let illum = moon_illum_pct(mid);
    if illum < MOON_ILLUM_MIN_PCT {
        return (false, illum);
    }
    let observer = Observer::new(lat_deg, lon_deg);
    let present = observer_moon_altitude_deg(&observer, mid) > 0.0;
    (present, illum)
}

fn night_bg_key(date: chrono::NaiveDate) -> String {
    format!("night_bg@{date}")
}

pub struct NightTimelinePlot {
    pub rows: Vec<NightTimelineRow>,
    pub local_tz: Tz,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub reset_view: bool,
    pub clicked_date: Option<chrono::NaiveDate>,
    /// Updated each frame from plot bounds (for ephemeris prefetch).
    pub view_max_y: f64,
}

fn date_to_y(date: chrono::NaiveDate) -> f64 {
    date.num_days_from_ce() as f64
}

fn y_to_date(y: f64) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::from_num_days_from_ce_opt(y.round() as i32)
}

fn today_y_bounds() -> (f64, f64) {
    let today = Utc::now().date_naive();
    let y = date_to_y(today);
    (y - 0.5, y + DEFAULT_VIEW_DAYS + 0.5)
}

fn bounds_look_like_epoch(ymin: f64, ymax: f64) -> bool {
    let today_y = date_to_y(Utc::now().date_naive());
    ymax < today_y - 365.0 * 50.0 || ymin < 1.0
}

/// Local midnight that falls inside the night of `date` (sunset on `date` → sunrise next day).
fn night_local_midnight(date: chrono::NaiveDate, tz: Tz) -> DateTime<Utc> {
    let next = date.succ_opt().unwrap_or(date);
    let naive = next.and_hms_opt(0, 0, 0).unwrap();
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(earliest, _) => earliest.with_timezone(&Utc),
        chrono::LocalResult::None => {
            // DST spring-forward gap: nudge forward an hour.
            let nudged = naive + Duration::hours(1);
            match tz.from_local_datetime(&nudged) {
                chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
                chrono::LocalResult::Ambiguous(earliest, _) => earliest.with_timezone(&Utc),
                chrono::LocalResult::None => naive.and_utc(),
            }
        }
    }
}

/// Hours relative to the night's local midnight (negative = evening, positive = morning).
fn utc_to_night_x(utc: DateTime<Utc>, night_date: chrono::NaiveDate, tz: Tz) -> f64 {
    let midnight = night_local_midnight(night_date, tz);
    (utc - midnight).num_milliseconds() as f64 / MS_PER_HOUR
}

fn format_night_hour(x_hours: f64) -> String {
    let total_mins = (x_hours * 60.0).round() as i64;
    let day_mins = 24 * 60;
    let mins = ((total_mins % day_mins) + day_mins) % day_mins;
    format!("{:02}:{:02}", mins / 60, mins % 60)
}

fn segment_series_key(name: &str, date: chrono::NaiveDate, si: usize) -> String {
    format!("{name}@{date}#{si}")
}

fn row_intersects_y(row: &NightTimelineRow, ymin: f64, ymax: f64) -> bool {
    let y = date_to_y(row.date);
    let half = 0.5 * ROW_HEIGHT;
    y + half >= ymin && y - half <= ymax
}

/// X range for the visible Y span: earliest sunset … latest sunrise (asymmetric OK).
fn x_bounds_for_y_span(rows: &[NightTimelineRow], ymin: f64, ymax: f64, tz: Tz) -> (f64, f64) {
    let mut sunset_min = f64::INFINITY;
    let mut sunrise_max = f64::NEG_INFINITY;
    for row in rows.iter().filter(|r| row_intersects_y(r, ymin, ymax)) {
        let sunset_x = utc_to_night_x(row.night.night_start_ms, row.date, tz);
        let sunrise_x = utc_to_night_x(row.night.night_end_ms, row.date, tz);
        sunset_min = sunset_min.min(sunset_x);
        sunrise_max = sunrise_max.max(sunrise_x);
    }
    if !sunset_min.is_finite() || !sunrise_max.is_finite() || sunrise_max <= sunset_min {
        return (-FALLBACK_NIGHT_HALF_HOURS, FALLBACK_NIGHT_HALF_HOURS);
    }
    (sunset_min, sunrise_max)
}

fn clamp_y_span(ymin: f64, ymax: f64) -> (f64, f64) {
    let mut ymin = ymin;
    let mut ymax = ymax;
    if !ymin.is_finite() || !ymax.is_finite() || ymax <= ymin {
        return today_y_bounds();
    }
    let mut span = ymax - ymin;
    if span < MIN_VIEW_DAYS {
        let mid = 0.5 * (ymin + ymax);
        ymin = mid - 0.5 * MIN_VIEW_DAYS;
        ymax = mid + 0.5 * MIN_VIEW_DAYS;
        span = MIN_VIEW_DAYS;
    } else if span > MAX_VIEW_DAYS {
        let mid = 0.5 * (ymin + ymax);
        ymin = mid - 0.5 * MAX_VIEW_DAYS;
        ymax = mid + 0.5 * MAX_VIEW_DAYS;
    }
    let _ = span;
    (ymin, ymax)
}

impl egui::Widget for &mut NightTimelinePlot {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        self.clicked_date = None;

        if self.rows.is_empty() {
            ui.label("No cached nights — ephemeris is loading or pan to extend the range.");
            return ui.response();
        }

        let local_tz = self.local_tz;

        let date_by_id: HashMap<Id, chrono::NaiveDate> = self
            .rows
            .iter()
            .flat_map(|row| {
                let bg = (Id::new(night_bg_key(row.date)), row.date);
                let objs = row.segments.segments.iter().flat_map(move |(name, segs)| {
                    let date = row.date;
                    (0..segs.len())
                        .map(move |si| (Id::new(segment_series_key(name, date, si)), date))
                });
                std::iter::once(bg).chain(objs)
            })
            .collect();

        let tip_rows: HashMap<String, String> = build_tip_map(&self.rows, local_tz);

        let hour_formatter =
            |mark: GridMark, _range: &std::ops::RangeInclusive<f64>| format_night_hour(mark.value);

        let x_grid = |input: GridInput| {
            let mut marks = Vec::new();
            let start = input.bounds.0.floor() as i64;
            let end = input.bounds.1.ceil() as i64;
            for h in start..=end {
                marks.push(GridMark {
                    value: h as f64,
                    step_size: 1.0,
                });
            }
            marks
        };

        let y_grid = |input: GridInput| {
            let mut marks = Vec::new();
            let start = input.bounds.0.ceil() as i64;
            let end = input.bounds.1.floor() as i64;
            let span = (end - start).max(1);
            let step = if span > 40 {
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

        let date_formatter = |mark: GridMark, _range: &std::ops::RangeInclusive<f64>| {
            y_to_date(mark.value)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        };

        let tips = tip_rows.clone();
        let label_fmt = move |name: &str, _value: &PlotPoint| {
            tips.get(name).cloned().unwrap_or_else(|| name.to_string())
        };

        let rows = self.rows.clone();
        let reset_view = self.reset_view;
        if reset_view {
            self.reset_view = false;
        }

        // X locked; Y pan/zoom handled manually so we can clamp the day span.
        let plot = Plot::new("night_timeline")
            .height(ui.available_height().max(220.0))
            .width(ui.available_width())
            .allow_zoom(false)
            .allow_drag(Vec2b::new(false, true))
            .allow_scroll(false)
            .allow_axis_zoom_drag(Vec2b::new(false, false))
            .auto_bounds(false)
            .invert_y(true)
            .show_axes(Vec2b::new(true, true))
            .show_grid(true)
            .sense(Sense::click_and_drag())
            .custom_x_axes(vec![
                AxisHints::new_x()
                    .label("Local night hours")
                    .formatter(hour_formatter),
            ])
            .custom_y_axes(vec![
                AxisHints::new_y().label("Date").formatter(date_formatter),
            ])
            .x_grid_spacer(x_grid)
            .y_grid_spacer(y_grid)
            .label_formatter(label_fmt);

        let mut view_max_y = f64::NAN;

        let plot_response = plot.show(ui, |plot_ui| {
            let bounds = plot_ui.plot_bounds();
            let mut ymin = bounds.min()[1];
            let mut ymax = bounds.max()[1];

            let need_reset = reset_view || bounds_look_like_epoch(ymin, ymax);
            if need_reset {
                let (y0, y1) = today_y_bounds();
                ymin = y0;
                ymax = y1;
            }

            // Wheel: pan dates. Ctrl+wheel: zoom Y (clamped 10–90 days). X never moves.
            if plot_ui.response().contains_pointer() {
                let (scroll, ctrl) = plot_ui
                    .ctx()
                    .input(|i| (i.smooth_scroll_delta.y, i.modifiers.ctrl));
                if scroll.abs() > f32::EPSILON {
                    if ctrl {
                        let zoom = (0.01 * scroll).exp();
                        plot_ui.zoom_bounds_around_hovered(Vec2::new(1.0, zoom));
                        let b = plot_ui.plot_bounds();
                        ymin = b.min()[1];
                        ymax = b.max()[1];
                    } else {
                        // Screen scroll down → later dates (toward bottom with invert_y).
                        let span = (ymax - ymin).max(1.0);
                        let dy = -(scroll as f64) / plot_ui.response().rect.height() as f64 * span;
                        ymin += dy;
                        ymax += dy;
                    }
                }
            }

            (ymin, ymax) = clamp_y_span(ymin, ymax);
            let (xmin, xmax) = x_bounds_for_y_span(&rows, ymin, ymax, local_tz);
            plot_ui.set_plot_bounds(PlotBounds::from_min_max([xmin, ymin], [xmax, ymax]));

            view_max_y = ymax;

            for row in &rows {
                if !row_intersects_y(row, ymin, ymax) {
                    continue;
                }
                let y = date_to_y(row.date);
                // Night first (behind). Hover disabled so object bars keep tip priority.
                draw_night_background(plot_ui, row, y, local_tz);
                draw_object_segments(plot_ui, row, y, local_tz, &tip_rows);
            }
        });

        // Night-row tip when not over an object (night bars use allow_hover(false)).
        if plot_response.hovered_plot_item.is_none() {
            if let Some(screen) = plot_response.response.hover_pos() {
                let value = plot_response.transform.value_from_position(screen);
                if let Some(date) = y_to_date(value.y) {
                    let key = night_bg_key(date);
                    if let Some(tip) = tip_rows.get(&key) {
                        if let Some(row) = rows.iter().find(|r| r.date == date) {
                            let x0 = utc_to_night_x(row.night.night_start_ms, row.date, local_tz);
                            let x1 = utc_to_night_x(row.night.night_end_ms, row.date, local_tz);
                            if value.x >= x0 && value.x <= x1 {
                                plot_response
                                    .response
                                    .clone()
                                    .show_tooltip_text(tip.as_str());
                            }
                        }
                    }
                }
            }
        }

        if plot_response.response.clicked() {
            if let Some(item_id) = plot_response.hovered_plot_item {
                if let Some(date) = date_by_id.get(&item_id) {
                    self.clicked_date = Some(*date);
                }
            } else if let Some(screen) = plot_response.response.interact_pointer_pos() {
                let value = plot_response.transform.value_from_position(screen);
                if let Some(date) = y_to_date(value.y) {
                    if rows.iter().any(|r| r.date == date) {
                        self.clicked_date = Some(date);
                    }
                }
            }
        }

        if view_max_y.is_finite() {
            self.view_max_y = view_max_y;
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(
                    "Scroll: pan dates · Ctrl+wheel: zoom days (10–90) · X locked to night hours",
                )
                .small()
                .weak(),
            );
        });

        plot_response.response
    }
}

fn draw_night_background(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    row: &NightTimelineRow,
    y: f64,
    tz: Tz,
) {
    let ni = &row.night;
    let xmin = utc_to_night_x(ni.night_start_ms, row.date, tz);
    let xmax = utc_to_night_x(ni.night_end_ms, row.date, tz);
    let width = (xmax - xmin).max(0.05);
    let fill = if row.moon_present {
        NIGHT_BG_MOONLIT
    } else {
        NIGHT_BG_DARK
    };
    // BarChart hit-tests the full rectangle (Polygon only hit vertices).
    // allow_hover(false): object bars above keep tip priority when overlapping.
    let bar = Bar::new(y, width)
        .base_offset(xmin)
        .width(ROW_HEIGHT)
        .horizontal()
        .fill(fill)
        .stroke(Stroke::new(0.5, Color32::from_gray(70)))
        .name(night_bg_key(row.date));
    plot_ui.bar_chart(BarChart::new(night_bg_key(row.date), vec![bar]).allow_hover(false));
}

fn draw_object_segments(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    row: &NightTimelineRow,
    y: f64,
    tz: Tz,
    tips: &HashMap<String, String>,
) {
    for (name, segs) in &row.segments.segments {
        let color = get_object_color(name);
        for (si, seg) in segs.iter().enumerate() {
            let mut iter = seg.iter();
            let Some(first) = iter.next() else {
                continue;
            };
            let last = iter.last().unwrap_or(first);
            let x0 = utc_to_night_x(first.utc_datetime, row.date, tz);
            let mut x1 = utc_to_night_x(last.utc_datetime, row.date, tz);
            if (x1 - x0).abs() < 1e-6 {
                x1 = x0 + SAMPLE_STEP_MINUTES as f64 / 60.0;
            }
            let key = segment_series_key(name, row.date, si);
            let tip = tips.get(&key).cloned().unwrap_or_else(|| key.clone());
            let bar = Bar::new(y, (x1 - x0).max(0.05))
                .base_offset(x0.min(x1))
                .width(OBJECT_BAR_HEIGHT)
                .horizontal()
                .fill(color)
                .stroke(Stroke::new(0.0, color))
                .name(key.clone());
            plot_ui.bar_chart(
                BarChart::new(key, vec![bar])
                    .element_formatter(Box::new(move |_bar, _chart| tip.clone())),
            );
        }
    }
}

fn build_tip_map(rows: &[NightTimelineRow], tz: Tz) -> HashMap<String, String> {
    let mut tips = HashMap::new();
    for row in rows {
        let quality = row.night_quality_label();
        let sunset = format_hm_local(row.night.night_start_ms, tz);
        let sunrise = format_hm_local(row.night.night_end_ms, tz);
        tips.insert(
            night_bg_key(row.date),
            format!(
                "{date}\nNight quality: {quality}\nSunset {sunset} → sunrise {sunrise}\nClick → Daily",
                date = row.date.format("%Y-%m-%d"),
                quality = quality,
                sunset = sunset,
                sunrise = sunrise,
            ),
        );
        for (name, segs) in &row.segments.segments {
            for (si, seg) in segs.iter().enumerate() {
                let mut iter = seg.iter();
                let Some(first) = iter.next() else {
                    continue;
                };
                let last = iter.last().unwrap_or(first);
                let key = segment_series_key(name, row.date, si);
                let start = format_hm_local(first.utc_datetime, tz);
                let end = format_hm_local(last.utc_datetime, tz);
                tips.insert(
                    key,
                    format!(
                        "{name}\n{date}\nNight quality: {quality}\nVisible {start} → {end}\nClick → Daily",
                        name = name,
                        date = row.date.format("%Y-%m-%d"),
                        quality = quality,
                        start = start,
                        end = end,
                    ),
                );
            }
        }
    }
    tips
}
