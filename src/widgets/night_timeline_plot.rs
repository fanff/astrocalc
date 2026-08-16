use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use chrono_tz::Tz;
use egui::{Color32, Id, Response, RichText, Sense, Stroke, Vec2, Vec2b};
use egui_plot::{AxisHints, GridInput, GridMark, Plot, PlotBounds, PlotPoint, Polygon, Text};
use std::collections::HashMap;

use crate::satellites::illumination::observer_sun_altitude_deg;
use crate::satellites::propagate::Observer;
use crate::solarsystemcalc::{
    NightInfo, ObjectPositionSegments, altitude_to_color, sky_darkness_from_sun_alt,
    sky_darkness_to_color,
};
use crate::timezone_util::{format_axis_local, format_axis_utc, format_utc_local_block};

const SAMPLE_STEP_MINUTES: i64 = 10;
const DEFAULT_VIEW_DAYS: f64 = 30.0;
const ROW_HEIGHT: f64 = 1.0;

pub struct NightTimelineRow {
    pub date: chrono::NaiveDate,
    pub night: NightInfo,
    pub segments: ObjectPositionSegments,
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

fn segment_series_key(name: &str, date: chrono::NaiveDate, si: usize, sample: usize) -> String {
    format!("{name}@{date}#{si}:{sample}")
}

fn altitude_half_height(alt_deg: f64) -> f64 {
    (alt_deg.clamp(0.0, 90.0) / 90.0) * 0.42 * ROW_HEIGHT
}

impl egui::Widget for &mut NightTimelinePlot {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        self.clicked_date = None;

        if self.rows.is_empty() {
            ui.label("No cached nights — ephemeris is loading or pan to extend the range.");
            return ui.response();
        }

        let local_tz = self.local_tz;
        let lat = self.lat_deg;
        let lon = self.lon_deg;
        let observer = Observer::new(lat, lon);

        let date_by_id: HashMap<Id, chrono::NaiveDate> = self
            .rows
            .iter()
            .flat_map(|row| {
                row.segments.segments.iter().flat_map(|(name, segs)| {
                    segs.iter().enumerate().flat_map(|(si, seg)| {
                        seg.iter()
                            .enumerate()
                            .map(|(i, _)| {
                                (
                                    Id::new(segment_series_key(name, row.date, si, i)),
                                    row.date,
                                )
                            })
                    })
                })
            })
            .collect();

        let tip_rows: HashMap<String, String> = build_tip_map(&self.rows, local_tz);

        let utc_formatter = |value: egui_plot::GridMark, _range: &std::ops::RangeInclusive<f64>| {
            format_axis_utc(value.value as i64)
        };
        let local_formatter =
            move |value: egui_plot::GridMark, _range: &std::ops::RangeInclusive<f64>| {
                format_axis_local(value.value as i64, local_tz)
            };

        let x_grid = |input: GridInput| {
            let mut marks = Vec::new();
            let start = input.bounds.0;
            let end = input.bounds.1;
            let Some(start_dt) = DateTime::from_timestamp_millis(start as i64) else {
                return marks;
            };
            let Some(end_dt) = DateTime::from_timestamp_millis(end as i64) else {
                return marks;
            };
            let mut t = start_dt
                - Duration::minutes(start_dt.minute() as i64)
                - Duration::seconds(start_dt.second() as i64)
                - Duration::nanoseconds(start_dt.nanosecond() as i64);
            if t < start_dt {
                t += Duration::hours(1);
            }
            while t <= end_dt {
                marks.push(GridMark {
                    value: t.timestamp_millis() as f64,
                    step_size: 3_600_000.0,
                });
                t += Duration::hours(1);
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
            tips.get(name)
                .cloned()
                .unwrap_or_else(|| name.to_string())
        };

        let rows = self.rows.clone();
        let reset_view = self.reset_view;
        if reset_view {
            self.reset_view = false;
        }

        let default_x = default_night_x_bounds(&rows);

        let plot = Plot::new("night_timeline")
            .height(ui.available_height().max(220.0))
            .width(ui.available_width())
            .allow_zoom(false)
            .allow_drag(true)
            .allow_scroll(false)
            .allow_axis_zoom_drag(Vec2b::new(true, true))
            .show_axes(Vec2b::new(true, true))
            .show_grid(true)
            .sense(Sense::click_and_drag())
            .custom_x_axes(vec![
                AxisHints::new_x().label("Night (UTC)").formatter(utc_formatter),
                AxisHints::new_x().label("Local").formatter(local_formatter),
            ])
            .custom_y_axes(vec![
                AxisHints::new_y().label("Date").formatter(date_formatter),
            ])
            .x_grid_spacer(x_grid)
            .label_formatter(label_fmt);

        let mut view_max_y = f64::NAN;

        let plot_response = plot.show(ui, |plot_ui| {
            let bounds = plot_ui.plot_bounds();
            let mut xmin = bounds.min()[0];
            let mut xmax = bounds.max()[0];
            let mut ymin = bounds.min()[1];
            let mut ymax = bounds.max()[1];

            let need_reset = reset_view || bounds_look_like_epoch(ymin, ymax);
            if need_reset {
                let (y0, y1) = today_y_bounds();
                ymin = y0;
                ymax = y1;
                xmin = default_x.0;
                xmax = default_x.1;
            }

            plot_ui.set_plot_bounds(PlotBounds::from_min_max([xmin, ymin], [xmax, ymax]));

            if plot_ui.response().contains_pointer() {
                let scroll = plot_ui.ctx().input(|i| i.smooth_scroll_delta.y);
                if scroll.abs() > f32::EPSILON {
                    let ctrl = plot_ui.ctx().input(|i| i.modifiers.ctrl);
                    let zoom = (0.01 * scroll).exp();
                    if ctrl {
                        plot_ui.zoom_bounds_around_hovered(Vec2::new(1.0, zoom));
                    } else {
                        plot_ui.zoom_bounds_around_hovered(Vec2::new(zoom, 1.0));
                    }
                }
            }

            view_max_y = plot_ui.plot_bounds().max()[1];

            for row in &rows {
                let y = date_to_y(row.date);
                draw_night_background(plot_ui, row, y, &observer);
                draw_object_segments(plot_ui, row, y);
            }
        });

        if plot_response.response.clicked() {
            if let Some(item_id) = plot_response.hovered_plot_item {
                if let Some(date) = date_by_id.get(&item_id) {
                    self.clicked_date = Some(*date);
                }
            }
        }

        if view_max_y.is_finite() {
            self.view_max_y = view_max_y;
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Wheel: zoom night hours · Ctrl+wheel: zoom day rows · Drag: pan")
                    .small()
                    .weak(),
            );
        });

        plot_response.response
    }
}

fn default_night_x_bounds(rows: &[NightTimelineRow]) -> (f64, f64) {
    let today = Utc::now().date_naive();
    if let Some(row) = rows.iter().find(|r| r.date == today) {
        return (
            row.night.night_start_ms.timestamp_millis() as f64,
            row.night.night_end_ms.timestamp_millis() as f64,
        );
    }
    if let Some(row) = rows.first() {
        return (
            row.night.night_start_ms.timestamp_millis() as f64,
            row.night.night_end_ms.timestamp_millis() as f64,
        );
    }
    (0.0, 1.0)
}

fn draw_night_background(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    row: &NightTimelineRow,
    y: f64,
    observer: &Observer,
) {
    let ni = &row.night;
    let xmin = ni.night_start_ms.timestamp_millis() as f64;
    let xmax = ni.night_end_ms.timestamp_millis() as f64;
    let step_ms = SAMPLE_STEP_MINUTES * 60_000;
    let mut t_ms = xmin;

    while t_ms < xmax {
        let next_ms = (t_ms + step_ms as f64).min(xmax);
        let mid_ts = ((t_ms + next_ms) / 2.0) as i64;
        if let Some(dt) = DateTime::from_timestamp_millis(mid_ts) {
            let sun_alt = observer_sun_altitude_deg(observer, dt);
            let darkness = sky_darkness_from_sun_alt(sun_alt);
            let fill = sky_darkness_to_color(darkness);
            let points: egui_plot::PlotPoints<'_> = egui_plot::PlotPoints::from_iter(vec![
                [t_ms, y - 0.02],
                [t_ms, y + ROW_HEIGHT - 0.02],
                [next_ms, y + ROW_HEIGHT - 0.02],
                [next_ms, y - 0.02],
            ]);
            plot_ui.polygon(
                Polygon::new(format!("sky@{}", row.date), points)
                    .fill_color(fill)
                    .stroke(Stroke::new(0.0, fill)),
            );
        }
        t_ms = next_ms;
    }

    // Night row outline
    let outline: egui_plot::PlotPoints<'_> = egui_plot::PlotPoints::from_iter(vec![
        [xmin, y],
        [xmin, y + ROW_HEIGHT],
        [xmax, y + ROW_HEIGHT],
        [xmax, y],
    ]);
    plot_ui.polygon(
        Polygon::new(format!("night_outline@{}", row.date), outline)
            .fill_color(Color32::TRANSPARENT)
            .stroke(Stroke::new(0.5, Color32::from_gray(90))),
    );
}

fn draw_object_segments(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    row: &NightTimelineRow,
    y: f64,
) {
    let row_center = y + ROW_HEIGHT * 0.5;

    for (name, segs) in &row.segments.segments {
        for (si, seg) in segs.iter().enumerate() {
            let positions: Vec<_> = seg.iter().collect();
            if positions.is_empty() {
                continue;
            }
            for (i, pos) in positions.iter().enumerate() {
                let x0 = pos.utc_datetime.timestamp_millis() as f64;
                let x1 = if i + 1 < positions.len() {
                    positions[i + 1].utc_datetime.timestamp_millis() as f64
                } else {
                    x0 + SAMPLE_STEP_MINUTES as f64 * 60_000.0
                };
                let half = altitude_half_height(pos.altitude);
                let color = altitude_to_color(pos.altitude).gamma_multiply(0.82);
                let min_y = row_center - half;
                let max_y = row_center + half;
                let key = segment_series_key(name, row.date, si, i);
                let points: egui_plot::PlotPoints<'_> = egui_plot::PlotPoints::from_iter(vec![
                    [x0, min_y],
                    [x0, max_y],
                    [x1, max_y],
                    [x1, min_y],
                ]);
                plot_ui.polygon(
                    Polygon::new(key, points)
                        .fill_color(color)
                        .stroke(Stroke::new(0.0, color)),
                );
            }
        }
    }
}

fn build_tip_map(rows: &[NightTimelineRow], tz: Tz) -> HashMap<String, String> {
    let mut tips = HashMap::new();
    for row in rows {
        for (name, segs) in &row.segments.segments {
            for (si, seg) in segs.iter().enumerate() {
                for (i, pos) in seg.iter().enumerate() {
                    let key = segment_series_key(name, row.date, si, i);
                    let mag = if pos.magnitude.is_finite() && pos.magnitude < 90.0 {
                        format!("{:.1}", pos.magnitude)
                    } else {
                        "—".into()
                    };
                    tips.insert(
                        key,
                        format!(
                            "{name}\n{date}\n{time}\nAlt {alt:.1}° Az {az:.1}°\nMag {mag}\nClick → Daily",
                            name = name,
                            date = row.date.format("%Y-%m-%d"),
                            time = format_utc_local_block(pos.utc_datetime, tz),
                            alt = pos.altitude,
                            az = pos.azimuth,
                            mag = mag,
                        ),
                    );
                }
            }
        }
    }
    tips
}
