//! Offline vector basemap from embedded Natural Earth / France GeoJSON.

use egui::{Color32, Pos2, Response, Shape, Stroke, Ui, epaint::PathStroke};
use geojson::{FeatureCollection, GeoJson, Geometry, Value};
use walkers::{MapMemory, Plugin, Projector, lon_lat};

const COUNTRIES_GEOJSON: &str =
    include_str!("../../assets/vector_map/ne_110m_admin_0_countries.geojson");
const FRANCE_ADMIN_GEOJSON: &str = include_str!("../../assets/vector_map/france_admin1.geojson");
const PLACES_GEOJSON: &str = include_str!("../../assets/vector_map/places.geojson");

const ADMIN_MIN_ZOOM: f64 = 4.5;
const PLACES_MIN_ZOOM: f64 = 5.0;
const PLACE_LABEL_MIN_ZOOM: f64 = 6.0;

#[derive(Clone)]
struct PolyFeature {
    name: String,
    /// Exterior rings as lon/lat pairs (GeoJSON order).
    rings: Vec<Vec<[f64; 2]>>,
}

#[derive(Clone)]
struct Place {
    name: String,
    lon: f64,
    lat: f64,
    scalerank: f64,
}

/// Parsed vector layers painted by [`VectorBasemapPlugin`].
#[derive(Clone)]
pub struct VectorBasemap {
    countries: Vec<PolyFeature>,
    france_admin: Vec<PolyFeature>,
    places: Vec<Place>,
}

impl VectorBasemap {
    pub fn load() -> Self {
        Self {
            countries: parse_polygons(COUNTRIES_GEOJSON, &["NAME_EN", "NAME", "name", "ADMIN"]),
            france_admin: parse_polygons(FRANCE_ADMIN_GEOJSON, &["name", "nom", "NAME"]),
            places: parse_places(PLACES_GEOJSON),
        }
    }
}

/// walkers plugin that draws the vector basemap into the map viewport.
pub struct VectorBasemapPlugin<'a> {
    pub data: &'a VectorBasemap,
}

impl Plugin for VectorBasemapPlugin<'_> {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        response: &Response,
        projector: &Projector,
        map_memory: &MapMemory,
    ) {
        let zoom = map_memory.zoom();
        let painter = ui.painter().with_clip_rect(response.rect);
        let rect = response.rect;

        // Soft land fill + borders for countries.
        let fill = Color32::from_rgb(52, 72, 58);
        let border = Color32::from_rgb(160, 175, 165);
        for feat in &self.data.countries {
            for ring in &feat.rings {
                let pts = project_ring(projector, ring, rect);
                if pts.len() < 3 {
                    continue;
                }
                paint_closed_poly(&painter, pts, fill, PathStroke::new(1.0, border));
            }
        }

        if zoom >= ADMIN_MIN_ZOOM {
            let admin_stroke = PathStroke::new(1.2, Color32::from_rgb(210, 190, 120));
            let admin_fill = Color32::from_rgba_unmultiplied(90, 100, 70, 40);
            for feat in &self.data.france_admin {
                for ring in &feat.rings {
                    let pts = project_ring(projector, ring, rect);
                    if pts.len() < 3 {
                        continue;
                    }
                    paint_closed_poly(&painter, pts, admin_fill, admin_stroke.clone());
                }
            }
        }

        if zoom >= PLACES_MIN_ZOOM {
            for place in &self.data.places {
                // Hide low-importance places when zoomed out.
                if zoom < PLACE_LABEL_MIN_ZOOM && place.scalerank > 2.0 {
                    continue;
                }
                let screen = projector
                    .project(lon_lat(place.lon, place.lat))
                    .to_pos2();
                if !rect.contains(screen) {
                    continue;
                }
                let r = if place.scalerank <= 1.0 { 3.5 } else { 2.5 };
                painter.circle_filled(screen, r, Color32::from_rgb(230, 220, 180));
                painter.circle_stroke(screen, r, Stroke::new(1.0, Color32::from_gray(40)));
                if zoom >= PLACE_LABEL_MIN_ZOOM {
                    painter.text(
                        screen + egui::vec2(5.0, -4.0),
                        egui::Align2::LEFT_BOTTOM,
                        &place.name,
                        egui::FontId::proportional(11.0),
                        Color32::from_gray(230),
                    );
                }
            }
        }

        if zoom >= PLACE_LABEL_MIN_ZOOM + 1.0 {
            for feat in &self.data.france_admin {
                if feat.name.is_empty() || feat.rings.is_empty() {
                    continue;
                }
                if let Some(c) = ring_centroid_lonlat(&feat.rings[0]) {
                    let screen = projector.project(lon_lat(c[0], c[1])).to_pos2();
                    if rect.contains(screen) {
                        painter.text(
                            screen,
                            egui::Align2::CENTER_CENTER,
                            &feat.name,
                            egui::FontId::proportional(10.0),
                            Color32::from_rgba_unmultiplied(230, 210, 140, 200),
                        );
                    }
                }
            }
        }
    }
}

fn paint_closed_poly(
    painter: &egui::Painter,
    pts: Vec<Pos2>,
    fill: Color32,
    stroke: PathStroke,
) {
    painter.add(Shape::Path(egui::epaint::PathShape {
        points: pts,
        closed: true,
        fill,
        stroke,
    }));
}

fn ring_centroid_lonlat(ring: &[[f64; 2]]) -> Option<[f64; 2]> {
    if ring.is_empty() {
        return None;
    }
    let n = ring.len() as f64;
    let (sx, sy) = ring.iter().fold((0.0, 0.0), |(sx, sy), c| (sx + c[0], sy + c[1]));
    Some([sx / n, sy / n])
}

fn project_ring(projector: &Projector, ring: &[[f64; 2]], clip: egui::Rect) -> Vec<Pos2> {
    let mut out = Vec::with_capacity(ring.len());
    // Decimate long rings for paint cost while keeping outline shape.
    let step = (ring.len() / 256).max(1);
    for (i, c) in ring.iter().enumerate() {
        if i % step != 0 && i + 1 != ring.len() {
            continue;
        }
        let p = projector.project(lon_lat(c[0], c[1])).to_pos2();
        if clip.expand(64.0).contains(p) || out.is_empty() {
            out.push(p);
        } else if let Some(last) = out.last() {
            if last.distance(p) > 2.0 {
                out.push(p);
            }
        }
    }
    out
}

fn parse_polygons(raw: &str, name_keys: &[&str]) -> Vec<PolyFeature> {
    let Ok(geo) = raw.parse::<GeoJson>() else {
        eprintln!("vector_basemap: failed to parse GeoJSON polygons");
        return Vec::new();
    };
    let Some(fc) = feature_collection(geo) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for feat in fc.features {
        let name = name_keys
            .iter()
            .find_map(|k| {
                feat.property(k)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        let Some(geom) = feat.geometry.as_ref() else {
            continue;
        };
        let rings = geometry_rings(geom);
        if !rings.is_empty() {
            out.push(PolyFeature { name, rings });
        }
    }
    out
}

fn parse_places(raw: &str) -> Vec<Place> {
    let Ok(geo) = raw.parse::<GeoJson>() else {
        eprintln!("vector_basemap: failed to parse places GeoJSON");
        return Vec::new();
    };
    let Some(fc) = feature_collection(geo) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for feat in fc.features {
        let name = feat
            .property("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let scalerank = feat
            .property("scalerank")
            .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)))
            .unwrap_or(5.0);
        let Some(Geometry { value, .. }) = feat.geometry.as_ref() else {
            continue;
        };
        if let Value::Point(coords) = value {
            if coords.len() >= 2 {
                out.push(Place {
                    name,
                    lon: coords[0],
                    lat: coords[1],
                    scalerank,
                });
            }
        }
    }
    out
}

fn feature_collection(geo: GeoJson) -> Option<FeatureCollection> {
    match geo {
        GeoJson::FeatureCollection(fc) => Some(fc),
        GeoJson::Feature(f) => Some(FeatureCollection {
            bbox: None,
            features: vec![f],
            foreign_members: None,
        }),
        GeoJson::Geometry(_) => None,
    }
}

fn geometry_rings(geom: &Geometry) -> Vec<Vec<[f64; 2]>> {
    match &geom.value {
        Value::Polygon(poly) => poly
            .first()
            .map(|exterior| vec![coords_to_ring(exterior)])
            .unwrap_or_default(),
        Value::MultiPolygon(mp) => mp
            .iter()
            .filter_map(|poly| poly.first().map(coords_to_ring))
            .collect(),
        _ => Vec::new(),
    }
}

fn coords_to_ring(coords: &Vec<Vec<f64>>) -> Vec<[f64; 2]> {
    coords
        .iter()
        .filter_map(|c| {
            if c.len() >= 2 {
                Some([c[0], c[1]])
            } else {
                None
            }
        })
        .collect()
}
