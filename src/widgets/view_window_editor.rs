//! Interactive polar editor for visibility zones (`ViewWindow`).

use egui::{
    Color32, Pos2, Response, Sense, Shape, Stroke, Ui, Vec2, epaint::PathStroke,
};

use crate::config::{ViewWindow, normalize_az};
use crate::widgets::sky_polar::{HORIZON_R, arc_points, az_alt_to_xy, circle_points, xy_to_az_alt};

const MIN_ALT_SPAN: f64 = 2.0;
const MIN_AZ_SPAN: f64 = 5.0;
const HANDLE_HIT_PX: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewWindowCorner {
    #[default]
    MinAzMinAlt,
    MinAzMaxAlt,
    MaxAzMinAlt,
    MaxAzMaxAlt,
}

impl ViewWindowCorner {
    fn all() -> [ViewWindowCorner; 4] {
        [
            ViewWindowCorner::MinAzMinAlt,
            ViewWindowCorner::MinAzMaxAlt,
            ViewWindowCorner::MaxAzMinAlt,
            ViewWindowCorner::MaxAzMaxAlt,
        ]
    }

    fn az_alt(self, vw: &ViewWindow) -> (f64, f64) {
        match self {
            ViewWindowCorner::MinAzMinAlt => (vw.min_az_deg, vw.min_alt_deg),
            ViewWindowCorner::MinAzMaxAlt => (vw.min_az_deg, vw.max_alt_deg),
            ViewWindowCorner::MaxAzMinAlt => (vw.max_az_deg, vw.min_alt_deg),
            ViewWindowCorner::MaxAzMaxAlt => (vw.max_az_deg, vw.max_alt_deg),
        }
    }

    fn apply(self, vw: &mut ViewWindow, az: f64, alt: f64) {
        let az = normalize_az(az);
        let alt = alt.clamp(0.0, 90.0);
        match self {
            ViewWindowCorner::MinAzMinAlt => {
                vw.min_az_deg = az;
                vw.min_alt_deg = alt;
            }
            ViewWindowCorner::MinAzMaxAlt => {
                vw.min_az_deg = az;
                vw.max_alt_deg = alt;
            }
            ViewWindowCorner::MaxAzMinAlt => {
                vw.max_az_deg = az;
                vw.min_alt_deg = alt;
            }
            ViewWindowCorner::MaxAzMaxAlt => {
                vw.max_az_deg = az;
                vw.max_alt_deg = alt;
            }
        }
        if vw.min_alt_deg > vw.max_alt_deg {
            std::mem::swap(&mut vw.min_alt_deg, &mut vw.max_alt_deg);
        }
        vw.clamp_alts(MIN_ALT_SPAN);
        ensure_az_span(vw);
    }
}

fn ensure_az_span(vw: &mut ViewWindow) {
    if vw.az_span_deg() >= MIN_AZ_SPAN {
        return;
    }
    vw.max_az_deg = normalize_az(vw.min_az_deg + MIN_AZ_SPAN);
}

/// Persistent drag/selection state owned by the app.
#[derive(Default)]
pub struct ViewWindowEditorState {
    pub selected: Option<usize>,
    pub drag_corner: Option<ViewWindowCorner>,
}

pub struct ViewWindowEditor<'a> {
    pub zones: &'a mut Vec<ViewWindow>,
    pub state: &'a mut ViewWindowEditorState,
    /// Polar canvas side length in pixels (clamped internally).
    pub canvas_side: f32,
}

impl egui::Widget for ViewWindowEditor<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            if ui.button("Add zone").clicked() {
                self.zones.push(ViewWindow::default_zone());
                self.state.selected = Some(self.zones.len() - 1);
                self.state.drag_corner = None;
            }
            if let Some(idx) = self.state.selected {
                if ui.button("Delete zone").clicked() && idx < self.zones.len() {
                    self.zones.remove(idx);
                    self.state.selected = if self.zones.is_empty() {
                        None
                    } else {
                        Some(idx.min(self.zones.len() - 1))
                    };
                    self.state.drag_corner = None;
                }
            }
        });

        let side = self
            .canvas_side
            .clamp(160.0, 320.0)
            .min(ui.available_width().max(160.0));
        let canvas_size = Vec2::splat(side);
        let mut outer_response = ui.allocate_response(Vec2::ZERO, Sense::hover());

        ui.horizontal(|ui| {
            let (response, painter) = ui.allocate_painter(canvas_size, Sense::click_and_drag());
            outer_response = response.clone();
            let rect = response.rect;
            let center = rect.center();
            let scale = (rect.width().min(rect.height()) * 0.45) / HORIZON_R as f32;

            let to_screen = |xy: [f64; 2]| -> Pos2 {
                Pos2::new(
                    center.x + xy[0] as f32 * scale,
                    center.y - xy[1] as f32 * scale,
                )
            };
            let from_screen = |pos: Pos2| -> [f64; 2] {
                [
                    ((pos.x - center.x) / scale) as f64,
                    ((center.y - pos.y) / scale) as f64,
                ]
            };

            painter.rect_filled(rect, 4.0, Color32::from_gray(28));

            for (r, dashed) in [(30.0_f64, true), (60.0, true), (90.0, false)] {
                let pts: Vec<Pos2> = circle_points(r, 96).into_iter().map(to_screen).collect();
                let stroke = PathStroke::new(
                    if dashed { 1.0 } else { 1.5 },
                    Color32::from_gray(if dashed { 90 } else { 160 }),
                );
                painter.add(Shape::line(pts, stroke));
            }
            painter.line_segment(
                [to_screen([0.0, -HORIZON_R]), to_screen([0.0, HORIZON_R])],
                Stroke::new(1.0, Color32::from_gray(120)),
            );
            painter.line_segment(
                [to_screen([-HORIZON_R, 0.0]), to_screen([HORIZON_R, 0.0])],
                Stroke::new(1.0, Color32::from_gray(120)),
            );

            let label = |text: &str, xy: [f64; 2]| {
                painter.text(
                    to_screen(xy),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(16.0),
                    Color32::WHITE,
                );
            };
            label("N", [0.0, HORIZON_R + 6.0]);
            label("E", [-HORIZON_R - 6.0, 0.0]);
            label("S", [0.0, -HORIZON_R - 6.0]);
            label("W", [HORIZON_R + 6.0, 0.0]);

            let zone_colors = [
                Color32::from_rgba_unmultiplied(80, 160, 255, 90),
                Color32::from_rgba_unmultiplied(80, 220, 140, 90),
                Color32::from_rgba_unmultiplied(240, 180, 60, 90),
                Color32::from_rgba_unmultiplied(220, 100, 180, 90),
            ];

            for (i, vw) in self.zones.iter().enumerate() {
                if !vw.is_valid() {
                    continue;
                }
                let fill = zone_colors[i % zone_colors.len()];
                let selected = self.state.selected == Some(i);
                let outline = PathStroke::new(
                    if selected { 2.5 } else { 1.5 },
                    if selected {
                        Color32::WHITE
                    } else {
                        Color32::from_gray(200)
                    },
                );
                let poly = sector_polygon(vw);
                let screen_pts: Vec<Pos2> = poly.into_iter().map(to_screen).collect();
                if screen_pts.len() >= 3 {
                    // Annular sectors are not always convex; fill by triangle fan strips.
                    paint_annulus(&painter, &screen_pts, fill, outline);
                }
            }

            if let Some(idx) = self.state.selected {
                if let Some(vw) = self.zones.get(idx) {
                    for corner in ViewWindowCorner::all() {
                        let (az, alt) = corner.az_alt(vw);
                        let p = to_screen(az_alt_to_xy(az, alt));
                        painter.circle_filled(p, 6.0, Color32::YELLOW);
                        painter.circle_stroke(p, 6.0, Stroke::new(1.0, Color32::BLACK));
                    }
                }
            }

            if response.drag_started() {
                self.state.drag_corner = None;
                if let Some(pointer) = response.interact_pointer_pos() {
                    if let Some(idx) = self.state.selected {
                        if let Some(vw) = self.zones.get(idx) {
                            for corner in ViewWindowCorner::all() {
                                let (az, alt) = corner.az_alt(vw);
                                let hp = to_screen(az_alt_to_xy(az, alt));
                                if hp.distance(pointer) <= HANDLE_HIT_PX {
                                    self.state.drag_corner = Some(corner);
                                    break;
                                }
                            }
                        }
                    }
                    if self.state.drag_corner.is_none() {
                        let xy = from_screen(pointer);
                        let (az, alt) = xy_to_az_alt(xy[0], xy[1]);
                        let mut hit = None;
                        for (i, vw) in self.zones.iter().enumerate().rev() {
                            if vw.contains(az, alt) {
                                hit = Some(i);
                                break;
                            }
                        }
                        self.state.selected = hit;
                    }
                }
            }

            if response.dragged() {
                if let (Some(idx), Some(corner)) = (self.state.selected, self.state.drag_corner) {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        let xy = from_screen(pointer);
                        let (az, alt) = xy_to_az_alt(xy[0], xy[1]);
                        if let Some(vw) = self.zones.get_mut(idx) {
                            corner.apply(vw, az, alt);
                        }
                    }
                }
            }

            if response.drag_stopped() {
                self.state.drag_corner = None;
            }

            ui.vertical(|ui| {
                ui.set_min_width(180.0);
                ui.label("Zones");
                egui::ScrollArea::vertical()
                    .max_height(canvas_size.y)
                    .show(ui, |ui| {
                        let mut delete_idx = None;
                        for (i, vw) in self.zones.iter().enumerate() {
                            let selected = self.state.selected == Some(i);
                            ui.horizontal(|ui| {
                                let label = format!("Zone {}", i + 1);
                                if ui.selectable_label(selected, label).clicked() {
                                    self.state.selected = Some(i);
                                }
                                if ui.small_button("×").clicked() {
                                    delete_idx = Some(i);
                                }
                            });
                            ui.label(vw.to_string());
                            if !vw.is_valid() {
                                ui.colored_label(Color32::RED, "invalid");
                            }
                            ui.separator();
                        }
                        if let Some(i) = delete_idx {
                            self.zones.remove(i);
                            self.state.selected = if self.zones.is_empty() {
                                None
                            } else {
                                Some(i.min(self.zones.len() - 1))
                            };
                            self.state.drag_corner = None;
                        }
                    });
            });
        });

        outer_response
    }
}

/// `screen_pts` is outer arc then reversed inner arc (closed ring).
fn paint_annulus(
    painter: &egui::Painter,
    screen_pts: &[Pos2],
    fill: Color32,
    outline: PathStroke,
) {
    let n = screen_pts.len();
    if n < 4 || n % 2 != 0 {
        painter.add(Shape::closed_line(screen_pts.to_vec(), outline));
        return;
    }
    let half = n / 2;
    // Pair outer[i] with corresponding inner (stored reversed at end).
    for i in 0..half - 1 {
        let o0 = screen_pts[i];
        let o1 = screen_pts[i + 1];
        let i1 = screen_pts[n - 1 - (i + 1)];
        let i0 = screen_pts[n - 1 - i];
        painter.add(Shape::convex_polygon(
            vec![o0, o1, i1, i0],
            fill,
            PathStroke::NONE,
        ));
    }
    painter.add(Shape::closed_line(screen_pts.to_vec(), outline));
}

fn sector_polygon(vw: &ViewWindow) -> Vec<[f64; 2]> {
    let wrap = vw.wraps_north();
    let steps = ((vw.az_span_deg() / 3.0).ceil() as usize).clamp(8, 72);
    let mut outer = arc_points(vw.min_az_deg, vw.max_az_deg, vw.min_alt_deg, wrap, steps);
    let mut inner = arc_points(vw.min_az_deg, vw.max_az_deg, vw.max_alt_deg, wrap, steps);
    inner.reverse();
    outer.append(&mut inner);
    outer
}
