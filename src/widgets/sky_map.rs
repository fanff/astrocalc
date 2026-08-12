use egui::{Align2, Color32, Id, PopupAnchor, Response, RichText};
use egui_plot::{Line, Plot, PlotBounds, PlotPoint, PlotPoints, PlotTransform, PlotUi, Text};
use chrono_tz::Tz;

use crate::solarsystemcalc::{ObjectPosition, ObjectPositionSegments, get_object_color};
use crate::timezone_util::format_utc_local_hm;
use crate::widgets::sky_polar::{az_alt_to_xy, circle_points};

/// Half-extent that fits horizon (r=90) plus N/E/S/W labels.
const VIEW_BASE: f64 = 110.0;
/// Maximum zoom-in factor relative to [`VIEW_BASE`].
const MAX_ZOOM: f64 = 3.0;
/// How far the view center may drift from origin (plot units).
const MAX_PAN: f64 = 25.0;

const RADAR_SERIES: &str = "radar_chart_labels";

pub struct SkyMapPlot {
    pub op_segs: ObjectPositionSegments,
    pub local_tz: Tz,
}
impl SkyMapPlot {
    pub fn new() -> Self {
        Self {
            op_segs: ObjectPositionSegments::new(),
            local_tz: Tz::UTC,
        }
    }
}

impl egui::Widget for &mut SkyMapPlot {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        // Fit a square into the cell without forcing the layout taller than available
        // (view_aspect(1) + a wide strip cell was expanding height to match width).
        let avail = ui.available_size();
        let side = avail.x.min(avail.y).clamp(140.0, 520.0);

        let plot = Plot::new("sky_map")
            .width(side)
            .height(side)
            .data_aspect(1.0)
            .show_axes(false)
            .show_grid(false)
            .show_x(false)
            .show_y(false)
            .allow_boxed_zoom(false)
            .default_x_bounds(-VIEW_BASE, VIEW_BASE)
            .default_y_bounds(-VIEW_BASE, VIEW_BASE)
            .auto_bounds(false);

        let plot_response = plot.show(ui, |plot_ui| {
            draw_radar_chart(plot_ui);

            for opt_name in self.op_segs.segments.keys() {
                let object_color = get_object_color(opt_name);
                let object_segs = self.op_segs.segments.get(opt_name).unwrap();
                for pos_segment in object_segs {
                    let points: PlotPoints<'_> =
                        PlotPoints::from_iter(pos_segment.iter().map(|pos| {
                            az_alt_to_xy(pos.azimuth, pos.altitude)
                        }));
                    plot_ui.line(
                        Line::new(opt_name, points)
                            .color(object_color)
                            .width(1.0),
                    );
                }
            }

            clamp_sky_view(plot_ui);
        });

        show_track_tooltip(
            ui,
            &plot_response.response,
            &plot_response.transform,
            &self.op_segs,
            self.local_tz,
        );

        plot_response.response
    }
}

fn show_track_tooltip(
    ui: &egui::Ui,
    response: &Response,
    transform: &PlotTransform,
    segments: &ObjectPositionSegments,
    local_tz: Tz,
) {
    let Some(pointer) = response.hover_pos() else {
        return;
    };
    let interact_radius_sq = ui.style().interaction.interact_radius.powi(2);
    let Some((name, pos)) = nearest_hit(segments, transform, pointer, interact_radius_sq) else {
        return;
    };

    egui::Tooltip::always_open(
        ui.ctx().clone(),
        ui.layer_id(),
        Id::new("sky_map_track_tooltip"),
        PopupAnchor::Pointer,
    )
    .gap(12.0)
    .show(|ui| {
        ui.label(format!(
            "{}\n{}\nalt {:.1}\u{00b0}  az {:.1}\u{00b0}",
            name,
            format_utc_local_hm(pos.utc_datetime, local_tz),
            pos.altitude,
            pos.azimuth
        ));
    });
}

fn nearest_hit<'a>(
    segments: &'a ObjectPositionSegments,
    transform: &PlotTransform,
    pointer: egui::Pos2,
    max_dist_sq: f32,
) -> Option<(&'a str, &'a ObjectPosition)> {
    let mut best: Option<(&str, &ObjectPosition, f32)> = None;
    for (name, segs) in &segments.segments {
        for segment in segs {
            for pos in segment.iter() {
                let [x, y] = az_alt_to_xy(pos.azimuth, pos.altitude);
                let screen = transform.position_from_point(&PlotPoint::new(x, y));
                let d2 = screen.distance_sq(pointer);
                if d2 <= max_dist_sq && best.is_none_or(|(_, _, bd)| d2 < bd) {
                    best = Some((name.as_str(), pos, d2));
                }
            }
        }
    }
    best.map(|(n, p, _)| (n, p))
}

fn clamp_sky_view(plot_ui: &mut PlotUi<'_>) {
    let bounds = plot_ui.plot_bounds();
    let min_half = VIEW_BASE / MAX_ZOOM;
    let max_half = VIEW_BASE;

    let mut half = (bounds.width().max(bounds.height()) * 0.5).clamp(min_half, max_half);
    if !half.is_finite() {
        half = max_half;
    }

    let center = bounds.center();
    let cx = center.x.clamp(-MAX_PAN, MAX_PAN);
    let cy = center.y.clamp(-MAX_PAN, MAX_PAN);

    plot_ui.set_plot_bounds(PlotBounds::from_min_max(
        [cx - half, cy - half],
        [cx + half, cy + half],
    ));
}

pub fn draw_radar_chart(plot_ui: &mut PlotUi) {
    plot_ui.text(
        Text::new(
            RADAR_SERIES,
            PlotPoint::new(0.0, 90.0),
            RichText::new("N").size(25.0),
        )
        .anchor(Align2::CENTER_BOTTOM),
    );

    plot_ui.text(
        Text::new(
            RADAR_SERIES,
            PlotPoint::new(-90.0, 0.0),
            RichText::new("E").size(25.0),
        )
        .anchor(Align2::RIGHT_CENTER),
    );
    plot_ui.text(
        Text::new(
            RADAR_SERIES,
            PlotPoint::new(0.0, -90.0),
            RichText::new("S").size(25.0),
        )
        .anchor(Align2::CENTER_TOP),
    );
    plot_ui.text(
        Text::new(
            RADAR_SERIES,
            PlotPoint::new(90.0, 0.0),
            RichText::new("W").size(25.0),
        )
        .anchor(Align2::LEFT_CENTER),
    );
    for r in [30.0, 60.0, 90.0] {
        let circle_pts: PlotPoints<'_> = circle_points(r, 128).into();
        plot_ui.line(
            Line::new(RADAR_SERIES, circle_pts)
                .color(Color32::from_gray((r * 255.0 / 90.0) as u8))
                .style(if r == 90.0 {
                    egui_plot::LineStyle::Solid
                } else {
                    egui_plot::LineStyle::Dashed { length: 10.0 }
                }),
        );
    }
    plot_ui.line(
        Line::new(
            RADAR_SERIES,
            PlotPoints::from_iter([[0.0, -90.0], [0.0, 90.0]]),
        )
        .color(Color32::from_gray(192)),
    );
    plot_ui.line(
        Line::new(
            RADAR_SERIES,
            PlotPoints::from_iter([[-90.0, 0.0], [90.0, 0.0]]),
        )
        .color(Color32::from_gray(192)),
    );
}
