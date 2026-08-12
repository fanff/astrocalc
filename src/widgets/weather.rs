use chrono_tz::Tz;
use egui::{Color32, Response, RichText, Sense, Ui, UiBuilder, Vec2b};
use egui_plot::{AxisHints, Legend, Line, Plot, PlotBounds, PlotPoints};

use crate::solarsystemcalc::NightInfo;
use crate::timezone_util::{format_axis_local, format_axis_utc};
use crate::weather_cache::{HourlyWeatherPoint, WeatherSnapshot};

/// Fixed left gutter — must be identical on every strip or the plots misalign on X.
const Y_LABEL_WIDTH: f32 = 84.0;
const PCT_STRIP_HEIGHT: f32 = 72.0;
const WIND_STRIP_HEIGHT: f32 = 78.0;

pub struct WeatherPanel<'a> {
    pub snapshot: Option<&'a WeatherSnapshot>,
    pub pending: bool,
    pub error: Option<&'a str>,
    pub night: Option<&'a NightInfo>,
    pub local_tz: Tz,
    pub force_refresh: &'a mut bool,
}

impl egui::Widget for WeatherPanel<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let local_tz = self.local_tz;
        ui.group(|ui| {
            if self.pending {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Fetching forecast...");
                });
                return;
            }

            if let Some(err) = self.error {
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::RED, format!("error: {err}"));
                    if ui.small_button("Refresh").clicked() {
                        *self.force_refresh = true;
                    }
                });
                return;
            }

            let Some(snap) = self.snapshot else {
                ui.horizontal(|ui| {
                    ui.label("No forecast loaded yet.");
                    if ui.small_button("Refresh").clicked() {
                        *self.force_refresh = true;
                    }
                });
                return;
            };

            ui.horizontal(|ui| {
                ui.label(format!(
                    "{:.2}, {:.2}",
                    snap.snapped.lat, snap.snapped.lon
                ));
                if ui.small_button("Refresh").clicked() {
                    *self.force_refresh = true;
                }
            });

            let night_hours = night_points(snap, self.night);
            if night_hours.is_empty() {
                ui.label("Unavailable");
                return;
            }

            let wind_vals: Vec<f64> = night_hours.iter().filter_map(|h| h.wind_speed).collect();
            let cloud_pts = points_from_hours(&night_hours, |h| h.cloud_cover);
            let humidity_pts = points_from_hours(&night_hours, |h| h.humidity);
            let wind_pts = points_from_hours(&night_hours, |h| h.wind_speed);
            let x_bounds = series_x_bounds(
                cloud_pts
                    .iter()
                    .chain(humidity_pts.iter())
                    .chain(wind_pts.iter()),
            );

            // Cloud + humidity share one 0–100% Y axis; wind keeps its own scale below.
            weather_pct_plot(ui, &cloud_pts, &humidity_pts, x_bounds);
            weather_wind_plot(ui, &wind_pts, x_bounds, local_tz, wind_y_range(&wind_vals));
        })
        .response
    }
}

fn points_from_hours(
    hours: &[&HourlyWeatherPoint],
    pick: impl Fn(&HourlyWeatherPoint) -> Option<f64>,
) -> Vec<[f64; 2]> {
    hours
        .iter()
        .filter_map(|h| pick(h).map(|v| [h.datetime.timestamp_millis() as f64, v]))
        .collect()
}

fn wind_y_range(values: &[f64]) -> (f64, f64) {
    let data_max = values.iter().copied().fold(0.0_f64, f64::max);
    let top = data_max.max(20.0).ceil();
    (0.0, top)
}

fn locked_weather_plot(id: &str, height: f32, width: f32) -> Plot<'static> {
    Plot::new(id)
        .height(height)
        .width(width)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_axis_zoom_drag(false)
        .sense(Sense::hover())
        .show_x(false)
        .show_y(false)
        .auto_bounds(false)
        .link_axis("weather_night_time", Vec2b::new(true, false))
}

/// Reserve a fixed-width label column that cannot grow with text length.
fn weather_gutter(ui: &mut Ui, height: f32, add_contents: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(Y_LABEL_WIDTH, height), Sense::hover());
    ui.allocate_new_ui(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            ui.set_min_size(egui::vec2(Y_LABEL_WIDTH, height));
            ui.set_max_size(egui::vec2(Y_LABEL_WIDTH, height));
            ui.set_clip_rect(rect);
            add_contents(ui);
        },
    );
}

fn weather_pct_plot(
    ui: &mut Ui,
    cloud: &[[f64; 2]],
    humidity: &[[f64; 2]],
    x_bounds: Option<(f64, f64)>,
) {
    ui.horizontal(|ui| {
        weather_gutter(ui, PCT_STRIP_HEIGHT, |ui| {
            ui.vertical(|ui| {
                ui.set_max_width(Y_LABEL_WIDTH);
                ui.with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
                    ui.label(RichText::new("cloud %").small().color(Color32::LIGHT_BLUE));
                    ui.label(
                        RichText::new("humidity %")
                            .small()
                            .color(Color32::LIGHT_GREEN),
                    );
                });
            });
        });

        let plot = locked_weather_plot("weather_pct", PCT_STRIP_HEIGHT, ui.available_width())
            .show_axes(false)
            .default_y_bounds(0.0, 100.0)
            .legend(Legend::default().position(egui_plot::Corner::RightTop));

        let cloud_pts = PlotPoints::from_iter(cloud.iter().copied());
        let humidity_pts = PlotPoints::from_iter(humidity.iter().copied());
        plot.show(ui, |plot_ui| {
            set_weather_bounds(plot_ui, x_bounds, 0.0, 100.0);
            plot_ui.line(
                Line::new("cloud %", cloud_pts)
                    .color(Color32::LIGHT_BLUE)
                    .width(1.5),
            );
            plot_ui.line(
                Line::new("humidity %", humidity_pts)
                    .color(Color32::LIGHT_GREEN)
                    .width(1.5),
            );
        });
    });
}

fn weather_wind_plot(
    ui: &mut Ui,
    wind: &[[f64; 2]],
    x_bounds: Option<(f64, f64)>,
    local_tz: Tz,
    y_range: (f64, f64),
) {
    let utc_fmt = |mark: egui_plot::GridMark, _range: &std::ops::RangeInclusive<f64>| {
        format_axis_utc(mark.value as i64)
    };
    let local_fmt = move |mark: egui_plot::GridMark, _range: &std::ops::RangeInclusive<f64>| {
        format_axis_local(mark.value as i64, local_tz)
    };
    let (y_min, y_max) = y_range;

    ui.horizontal(|ui| {
        weather_gutter(ui, WIND_STRIP_HEIGHT, |ui| {
            ui.label(RichText::new("wind km/h").small().color(Color32::LIGHT_RED));
        });

        let plot = locked_weather_plot("weather_wind", WIND_STRIP_HEIGHT, ui.available_width())
            .show_axes(Vec2b::new(true, false))
            .default_y_bounds(y_min, y_max)
            .custom_x_axes(vec![
                AxisHints::new_x().formatter(utc_fmt),
                AxisHints::new_x().formatter(local_fmt),
            ]);

        let points = PlotPoints::from_iter(wind.iter().copied());
        plot.show(ui, |plot_ui| {
            set_weather_bounds(plot_ui, x_bounds, y_min, y_max);
            plot_ui.line(
                Line::new("wind km/h", points)
                    .color(Color32::LIGHT_RED)
                    .width(1.5),
            );
        });
    });
}

fn set_weather_bounds(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    x_bounds: Option<(f64, f64)>,
    y_min: f64,
    y_max: f64,
) {
    if let Some((xmin, xmax)) = x_bounds {
        plot_ui.set_plot_bounds(PlotBounds::from_min_max([xmin, y_min], [xmax, y_max]));
    } else {
        plot_ui.set_plot_bounds_y(y_min..=y_max);
    }
}

fn series_x_bounds<'a>(points: impl Iterator<Item = &'a [f64; 2]>) -> Option<(f64, f64)> {
    let mut iter = points.map(|p| p[0]);
    let first = iter.next()?;
    let (mut xmin, mut xmax) = (first, first);
    for x in iter {
        xmin = xmin.min(x);
        xmax = xmax.max(x);
    }
    if !xmin.is_finite() || !xmax.is_finite() {
        return None;
    }
    if xmax <= xmin {
        let pad = 3_600_000.0; // 1h
        Some((xmin - pad, xmax + pad))
    } else {
        let pad = (xmax - xmin) * 0.02;
        Some((xmin - pad, xmax + pad))
    }
}

fn night_points<'a>(
    snap: &'a WeatherSnapshot,
    night: Option<&NightInfo>,
) -> Vec<&'a HourlyWeatherPoint> {
    // Only hours that overlap the selected night. No fallback to "latest" forecast days.
    let Some(n) = night else {
        return Vec::new();
    };
    snap.night_hours(n.night_start_ms, n.night_end_ms)
}
