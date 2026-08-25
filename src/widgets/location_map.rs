//! Location map: OSM online tiles with blank fallback when offline.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use egui::{Align2, Color32, Context, FontId, Response, Sense, Shape, Stroke, Ui, Vec2};
use walkers::{
    HeaderValue, HttpOptions, HttpTiles, Map, MapMemory, Plugin, Position, Projector, lon_lat,
    sources::OpenStreetMap,
};

use crate::config::ViewWindow;

/// OSM tile host used for a cheap reachability probe.
const OSM_PROBE_HOST: &str = "tile.openstreetmap.org:443";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapTileMode {
    /// Use OSM when reachable, otherwise no tiles.
    Auto,
    Online,
    Offline,
}

impl MapTileMode {
    pub fn label(self) -> &'static str {
        match self {
            MapTileMode::Auto => "Auto",
            MapTileMode::Online => "OSM",
            MapTileMode::Offline => "Offline",
        }
    }
}

/// Click-to-set location; marker is always drawn from `marker` (canonical lat/lon).
#[derive(Clone)]
pub struct LocationClickPlugin {
    pub marker: Position,
    pub pending_click: Option<Position>,
    pub view_windows: Vec<ViewWindow>,
    pub selected_window: Option<usize>,
}

impl LocationClickPlugin {
    pub fn new(lon: f64, lat: f64) -> Self {
        Self {
            marker: lon_lat(lon, lat),
            pending_click: None,
            view_windows: Vec::new(),
            selected_window: None,
        }
    }

    pub fn set_marker(&mut self, lon: f64, lat: f64) {
        self.marker = lon_lat(lon, lat);
    }

    pub fn set_view_windows(&mut self, windows: &[ViewWindow], selected: Option<usize>) {
        self.view_windows = windows.to_vec();
        self.selected_window = selected;
    }
}

impl Plugin for &mut LocationClickPlugin {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        response: &Response,
        projector: &Projector,
        _map_memory: &MapMemory,
    ) {
        if !response.changed() && response.clicked_by(egui::PointerButton::Primary) {
            self.pending_click = response
                .interact_pointer_pos()
                .map(|p| projector.unproject(p.to_vec2()));
        }

        let screen = projector.project(self.marker).to_pos2();
        if response.rect.contains(screen) {
            let painter = ui.painter().with_clip_rect(response.rect);
            let radius = 64.0;
            for (index, window) in self.view_windows.iter().enumerate() {
                let selected = self.selected_window == Some(index);
                let color = if selected {
                    Color32::from_rgb(255, 196, 64)
                } else {
                    Color32::from_rgb(72, 180, 255)
                };
                let fill = Color32::from_rgba_unmultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    if selected { 46 } else { 26 },
                );
                let azimuths = wedge_azimuths(window, 4.0);
                for pair in azimuths.windows(2) {
                    painter.add(Shape::convex_polygon(
                        vec![
                            screen,
                            screen + azimuth_vector(pair[0], radius),
                            screen + azimuth_vector(pair[1], radius),
                        ],
                        fill,
                        Stroke::NONE,
                    ));
                }

                let stroke = Stroke::new(if selected { 2.5_f32 } else { 1.5_f32 }, color);
                for azimuth in [window.min_az_deg, window.max_az_deg] {
                    let endpoint = screen + azimuth_vector(azimuth, radius);
                    painter.line_segment([screen, endpoint], stroke);
                    painter.text(
                        endpoint + azimuth_vector(azimuth, 8.0),
                        Align2::CENTER_CENTER,
                        format!("{azimuth:.0}°"),
                        FontId::proportional(11.0),
                        color,
                    );
                }
                let arc = azimuths
                    .iter()
                    .map(|azimuth| screen + azimuth_vector(*azimuth, radius))
                    .collect();
                painter.add(Shape::line(arc, stroke));
            }
            painter.circle_filled(screen, 6.0, Color32::from_rgb(30, 120, 255));
            painter.circle_stroke(screen, 6.0, egui::Stroke::new(1.5, Color32::WHITE));
        }
    }
}

/// Holds OSM tiles and map interaction state.
pub struct LocationMap {
    pub http_tiles: HttpTiles,
    pub map_memory: MapMemory,
    pub click: LocationClickPlugin,
    pub mode: MapTileMode,
    online_ok: Arc<AtomicBool>,
    probe_done: Arc<AtomicBool>,
}

impl LocationMap {
    pub fn new(egui_ctx: Context, lon: f64, lat: f64) -> Self {
        let mut map_memory = MapMemory::default();
        let _ = map_memory.set_zoom(5.5);

        let online_ok = Arc::new(AtomicBool::new(false));
        let probe_done = Arc::new(AtomicBool::new(false));
        start_osm_probe(online_ok.clone(), probe_done.clone());

        let http_options = HttpOptions {
            user_agent: Some(HeaderValue::from_static(
                "AstroCalc/0.1 (desktop observing planner)",
            )),
            ..HttpOptions::default()
        };

        Self {
            http_tiles: HttpTiles::with_options(OpenStreetMap, http_options, egui_ctx),
            map_memory,
            click: LocationClickPlugin::new(lon, lat),
            mode: MapTileMode::Auto,
            online_ok,
            probe_done,
        }
    }

    pub fn probe_finished(&self) -> bool {
        self.probe_done.load(Ordering::Relaxed)
    }

    pub fn osm_reachable(&self) -> bool {
        self.online_ok.load(Ordering::Relaxed)
    }

    /// Effective tile source after applying mode + probe.
    pub fn use_online(&self) -> bool {
        match self.mode {
            MapTileMode::Online => true,
            MapTileMode::Offline => false,
            MapTileMode::Auto => {
                if !self.probe_finished() {
                    false
                } else {
                    self.osm_reachable()
                }
            }
        }
    }

    pub fn status_label(&self) -> String {
        if self.use_online() {
            "Active: OSM".into()
        } else {
            "Active: offline (no tiles)".into()
        }
    }

    /// Recenter the map after switching to another saved profile.
    pub fn center_on(&mut self, lon: f64, lat: f64) {
        self.map_memory = MapMemory::default();
        let _ = self.map_memory.set_zoom(5.5);
        self.click.set_marker(lon, lat);
    }

    /// Draw the map filling `ui`'s available size (caller should constrain the region).
    /// Returns `true` if the location changed.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        lon: &mut f64,
        lat: &mut f64,
        view_windows: &[ViewWindow],
        selected_window: Option<usize>,
    ) -> bool {
        if !self.probe_finished() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }

        self.click.set_marker(*lon, *lat);
        self.click.set_view_windows(view_windows, selected_window);

        let desired = ui.available_size().max(Vec2::splat(160.0));
        let (rect, _response) = ui.allocate_exact_size(desired, Sense::hover());

        ui.painter()
            .rect_filled(rect, 2.0, Color32::from_rgb(28, 40, 55));
        ui.painter().rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0, Color32::from_gray(120)),
            egui::StrokeKind::Inside,
        );

        let use_online = self.use_online();

        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_min_size(rect.size());
        child.set_max_size(rect.size());

        let map = if use_online {
            Map::new(
                Some(&mut self.http_tiles),
                &mut self.map_memory,
                lon_lat(*lon, *lat),
            )
            .zoom_with_ctrl(false)
            .with_plugin(&mut self.click)
        } else {
            Map::new(None, &mut self.map_memory, lon_lat(*lon, *lat))
                .zoom_with_ctrl(false)
                .with_plugin(&mut self.click)
        };
        child.add(map);

        if use_online {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }

        if let Some(pos) = self.click.pending_click.take() {
            *lon = pos.x();
            *lat = pos.y();
            self.click.set_marker(*lon, *lat);
            true
        } else {
            false
        }
    }
}

fn azimuth_vector(azimuth_deg: f64, radius: f32) -> Vec2 {
    let radians = azimuth_deg.to_radians();
    Vec2::new(
        (radians.sin() as f32) * radius,
        -(radians.cos() as f32) * radius,
    )
}

fn wedge_azimuths(window: &ViewWindow, step_deg: f64) -> Vec<f64> {
    let span = window.az_span_deg();
    let steps = (span / step_deg).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|index| window.min_az_deg + span * index as f64 / steps as f64)
        .collect()
}

fn start_osm_probe(online_ok: Arc<AtomicBool>, probe_done: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("osm-probe".into())
        .spawn(move || {
            let ok = probe_osm_reachable();
            online_ok.store(ok, Ordering::Relaxed);
            probe_done.store(true, Ordering::Relaxed);
        })
        .ok();
}

fn probe_osm_reachable() -> bool {
    let Ok(mut addrs) = OSM_PROBE_HOST.to_socket_addrs() else {
        return false;
    };
    let Some(addr) = addrs.next() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(800)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_wedge_samples_across_north_in_increasing_order() {
        let samples = wedge_azimuths(&ViewWindow::new(350.0, 10.0, 5.0, 80.0), 4.0);
        assert_eq!(samples.first().copied(), Some(350.0));
        assert_eq!(samples.last().copied(), Some(370.0));
        assert!(samples.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
