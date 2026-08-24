use std::collections::HashMap;

use crate::solarsystemcalc::{
    NightInfo, ObjectPositionSegments, darken_for_bar_fill, get_object_color,
};
use crate::timezone_util::{format_axis_local, format_axis_utc, format_utc_local_block};
use chrono::{DateTime, Duration, Timelike, Utc};
use chrono_tz::Tz;
use egui::{Align2, Color32, Response, RichText, Sense, Vec2b};
use egui_plot::{AxisHints, Plot, PlotBounds, PlotPoint, Polygon, Text, VLine};

pub struct CalPlot {
    pub dateinfo: Option<NightInfo>,
    pub output_timezone: Tz,
    pub positions_map: ObjectPositionSegments,
    /// Display name → short type/description for bar labels.
    pub object_types: HashMap<String, String>,
}
impl CalPlot {
    pub fn new() -> Self {
        Self {
            dateinfo: None,
            output_timezone: Tz::UTC,
            positions_map: ObjectPositionSegments::new(),
            object_types: HashMap::new(),
        }
    }
}

fn representative_magnitude(segments: &[crate::solarsystemcalc::ObjectSegment]) -> f64 {
    segments
        .iter()
        .flat_map(|s| s.iter())
        .map(|p| p.magnitude)
        .fold(f64::INFINITY, f64::min)
}

/// Sort key: lower = brighter = higher on the Gantt. Moon uses illumination, not the stub magnitude.
fn gantt_sort_key(name: &str, segments: &[crate::solarsystemcalc::ObjectSegment]) -> f64 {
    if name == "Moon" {
        let illum = segments
            .iter()
            .flat_map(|s| s.iter())
            .map(|p| p.phase_ratio)
            .fold(0.0_f64, f64::max)
            .clamp(0.0, 100.0);
        // Full moon ~0 (top), new moon ~10 (fainter).
        return 10.0 * (1.0 - illum / 100.0);
    }
    representative_magnitude(segments)
}

fn moon_phase_label(illum_pct: f64) -> String {
    let pct = illum_pct.clamp(0.0, 100.0);
    let name = match pct {
        x if x < 5.0 => "New",
        x if x < 45.0 => "Crescent",
        x if x < 55.0 => "Quarter",
        x if x < 95.0 => "Gibbous",
        _ => "Full",
    };
    format!("{name} ({pct:.0}%)")
}

fn bar_label_text(
    name: &str,
    mag: f64,
    phase_pct: Option<f64>,
    type_label: &str,
    wide_enough: bool,
) -> String {
    if !wide_enough {
        return name.to_string();
    }
    if name == "Moon" {
        let phase = moon_phase_label(phase_pct.unwrap_or(0.0));
        return format!("{name}\n{phase}\n{type_label}");
    }
    let mag_s = if mag.is_finite() && mag < 90.0 {
        format!("{mag:.1}")
    } else {
        "—".into()
    };
    format!("{name}\nmag {mag_s}\n{type_label}")
}

impl egui::Widget for &mut CalPlot {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let output_timezone = self.output_timezone;
        let utc_formatter = |value: egui_plot::GridMark, _range: &std::ops::RangeInclusive<f64>| {
            format_axis_utc(value.value as i64)
        };
        let local_formatter =
            move |value: egui_plot::GridMark, _range: &std::ops::RangeInclusive<f64>| {
                format_axis_local(value.value as i64, output_timezone)
            };

        let x_grid = move |input: egui_plot::GridInput| {
            let mut marks = Vec::new();
            let start = input.bounds.0;
            let end = input.bounds.1;
            let Some(start_dt) = DateTime::from_timestamp_millis(start as i64) else {
                return marks;
            };
            let Some(end_dt) = DateTime::from_timestamp_millis(end as i64) else {
                return marks;
            };
            // Align to next UTC hour boundary at or after start.
            let mut t = start_dt
                - Duration::minutes(start_dt.minute() as i64)
                - Duration::seconds(start_dt.second() as i64)
                - Duration::nanoseconds(start_dt.nanosecond() as i64);
            if t < start_dt {
                t += Duration::hours(1);
            }
            while t <= end_dt {
                marks.push(egui_plot::GridMark {
                    value: t.timestamp_millis() as f64,
                    step_size: 3_600_000.0,
                });
                t += Duration::hours(1);
            }
            marks
        };

        let labelfmt = move |name: &str, value: &PlotPoint| {
            let Some(dt) = DateTime::from_timestamp_millis(value.x as i64) else {
                return name.to_string();
            };
            if name == "now" {
                return format!("Now\n{}", format_utc_local_block(dt, output_timezone));
            }
            format!("{}\n{}", name, format_utc_local_block(dt, output_timezone))
        };

        let plot = Plot::new("cal_plot")
            .width(ui.available_width())
            .height(ui.available_height().max(160.0))
            .show_axes(Vec2b::new(true, false))
            .show_grid(true)
            .show_x(false)
            .show_y(false)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .allow_axis_zoom_drag(false)
            .sense(Sense::hover())
            .auto_bounds(false)
            .custom_x_axes(vec![
                AxisHints::new_x().formatter(utc_formatter),
                AxisHints::new_x().formatter(local_formatter),
            ])
            .x_grid_spacer(x_grid)
            .label_formatter(labelfmt);

        plot.show(ui, |plot_ui| {
            let Some(ni) = self.dateinfo.as_ref() else {
                return;
            };
            let xmin = ni.night_start_ms.timestamp_millis() as f64;
            let xmax = ni.night_end_ms.timestamp_millis() as f64;

            plot_ui.vline(VLine::new("night_start", xmin));
            plot_ui.vline(VLine::new("night_end", xmax));

            // Faintest first (low y); brightest last (high y = top of chart).
            let mut rows: Vec<(String, f64)> = self
                .positions_map
                .segments
                .iter()
                .map(|(name, segs)| (name.clone(), gantt_sort_key(name, segs)))
                .collect();
            rows.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.0.cmp(&b.0))
            });

            let row_count = rows.len().max(1) as f64;
            plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                [xmin, -0.05],
                [xmax, row_count + 0.05],
            ));

            let mut obj_index = 0.0;
            for (name, row_mag) in rows {
                let Some(segments) = self.positions_map.segments.get(&name) else {
                    continue;
                };
                let line_color = get_object_color(&name);
                let fill = darken_for_bar_fill(line_color);
                let type_label = self
                    .object_types
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| "Object".into());

                for segment in segments {
                    let first_point = segment.iter().next().unwrap();
                    let last_point = segment.iter().last().unwrap();

                    let min_y = obj_index;
                    let max_y = obj_index + 1.0;
                    let min_x = first_point.utc_datetime.timestamp_millis() as f64;
                    let max_x = last_point.utc_datetime.timestamp_millis() as f64;
                    // ~45 minutes of night span → enough room for 3 lines of text.
                    let wide_enough = (max_x - min_x) >= 45.0 * 60_000.0;

                    let points: egui_plot::PlotPoints<'_> = egui_plot::PlotPoints::from_iter(vec![
                        [min_x, min_y],
                        [min_x, max_y],
                        [max_x, max_y],
                        [max_x, min_y],
                    ]);

                    // width 0: egui_plot auto-colors TRANSPARENT strokes, so zero width hides the border.
                    plot_ui.polygon(
                        Polygon::new(name.clone(), points)
                            .fill_color(fill)
                            .stroke(egui::Stroke::new(0.0, fill)),
                    );

                    let mid_x = (min_x + max_x) * 0.5;
                    let mid_y = obj_index + 0.5;
                    let mag = segment
                        .iter()
                        .map(|p| p.magnitude)
                        .fold(f64::INFINITY, f64::min)
                        .min(row_mag);
                    let phase_pct = if name == "Moon" {
                        Some(
                            segment
                                .iter()
                                .map(|p| p.phase_ratio)
                                .fold(0.0_f64, f64::max),
                        )
                    } else {
                        None
                    };
                    let label = bar_label_text(&name, mag, phase_pct, &type_label, wide_enough);
                    let font_size = if wide_enough { 10.0 } else { 11.0 };
                    plot_ui.text(
                        Text::new(
                            format!("{name}_label"),
                            PlotPoint::new(mid_x, mid_y),
                            RichText::new(label).size(font_size).strong(),
                        )
                        .color(Color32::WHITE)
                        .anchor(Align2::CENTER_CENTER),
                    );
                }
                obj_index += 1.0;
            }

            let now = Utc::now();
            if now >= ni.night_start_ms && now <= ni.night_end_ms {
                plot_ui.vline(
                    VLine::new("now", now.timestamp_millis() as f64)
                        .color(Color32::from_rgb(255, 210, 90))
                        .width(2.0)
                        .style(egui_plot::LineStyle::Dashed { length: 6.0 }),
                );
            }
        });
        ui.response()
    }
}
