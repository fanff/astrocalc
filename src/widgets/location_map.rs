//! Location map: OSM online tiles with offline vector basemap fallback.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use egui::{Color32, Context, Response, Sense, Stroke, Ui, Vec2};
use walkers::{
    HeaderValue, HttpOptions, HttpTiles, Map, MapMemory, Plugin, Position, Projector, lon_lat,
    sources::OpenStreetMap,
};

use crate::widgets::vector_basemap::{VectorBasemap, VectorBasemapPlugin};

/// OSM tile host used for a cheap reachability probe.
const OSM_PROBE_HOST: &str = "tile.openstreetmap.org:443";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapTileMode {
    /// Use OSM when reachable, otherwise vector basemap.
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
}

impl LocationClickPlugin {
    pub fn new(lon: f64, lat: f64) -> Self {
        Self {
            marker: lon_lat(lon, lat),
            pending_click: None,
        }
    }

    pub fn set_marker(&mut self, lon: f64, lat: f64) {
        self.marker = lon_lat(lon, lat);
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
            painter.circle_filled(screen, 6.0, Color32::from_rgb(30, 120, 255));
            painter.circle_stroke(screen, 6.0, egui::Stroke::new(1.5, Color32::WHITE));
        }
    }
}

/// Holds online OSM tiles + offline vector basemap and map interaction state.
pub struct LocationMap {
    pub http_tiles: HttpTiles,
    pub map_memory: MapMemory,
    pub click: LocationClickPlugin,
    pub mode: MapTileMode,
    vector: VectorBasemap,
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
            vector: VectorBasemap::load(),
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
                    // Prefer vector while probing (instant offline-capable UI).
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
            "Active: vector basemap".into()
        }
    }

    /// Draw the map filling `ui`'s available size (caller should constrain the region).
    /// Returns `true` if the location changed.
    pub fn show(&mut self, ui: &mut Ui, lon: &mut f64, lat: &mut f64) -> bool {
        if !self.probe_finished() {
            ui.ctx()
                .request_repaint_after(Duration::from_millis(250));
        }

        self.click.set_marker(*lon, *lat);

        let desired = ui.available_size().max(Vec2::splat(160.0));
        let (rect, _response) = ui.allocate_exact_size(desired, Sense::hover());

        // Ocean / empty background (vector land draws on top when offline).
        ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(28, 40, 55));
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
                .with_plugin(VectorBasemapPlugin {
                    data: &self.vector,
                })
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
