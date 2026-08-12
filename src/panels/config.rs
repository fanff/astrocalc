//! Config panel: observer location map + visibility zones + save to SQLite.

use egui::{Align, Color32, Layout, Ui, Vec2};

use crate::config::{AppSettings, ViewWindow};
use crate::models::AppSettingsRow;
use crate::widgets::location_map::{LocationMap, MapTileMode};
use crate::widgets::view_window_editor::{ViewWindowEditor, ViewWindowEditorState};
use diesel::{Connection, SqliteConnection};

/// Mutable inputs the Config panel edits.
pub struct ConfigPanel<'a> {
    pub lat: &'a mut f64,
    pub long: &'a mut f64,
    pub timezone_name: &'a str,
    pub database_url: &'a str,
    pub view_windows: &'a mut Vec<ViewWindow>,
    pub zone_editor: &'a mut ViewWindowEditorState,
    pub location_map: &'a mut LocationMap,
}

impl ConfigPanel<'_> {
    /// Draw the compact Config UI. Returns `true` if lat/lon changed via map click.
    pub fn show(self, ui: &mut Ui) -> bool {
        if self.zone_editor.selected.is_none() && !self.view_windows.is_empty() {
            self.zone_editor.selected = Some(0);
        }

        let settings = AppSettings {
            lat: *self.lat,
            lon: *self.long,
            view_windows: self.view_windows.clone(),
        };
        let can_save = settings.is_valid();

        ui.horizontal(|ui| {
            ui.label(format!("Location: {:.4}, {:.4}", *self.lat, *self.long));
            ui.separator();
            ui.label(format!("TZ: {}", self.timezone_name));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(can_save, |ui| {
                    if ui.button("Save").clicked() {
                        match SqliteConnection::establish(self.database_url) {
                            Ok(mut conn) => {
                                if let Err(e) = AppSettingsRow::upsert(&mut conn, &settings) {
                                    eprintln!("Failed to save settings: {e}");
                                }
                            }
                            Err(e) => eprintln!("Failed to open database: {e}"),
                        }
                    }
                });
            });
        });
        if !can_save {
            ui.colored_label(
                Color32::YELLOW,
                "Save disabled: need a valid location and at least one valid visibility zone",
            );
        }

        ui.horizontal(|ui| {
            ui.label("Map source:");
            ui.radio_value(&mut self.location_map.mode, MapTileMode::Auto, "Auto");
            ui.radio_value(&mut self.location_map.mode, MapTileMode::Online, "OSM (online)");
            ui.radio_value(
                &mut self.location_map.mode,
                MapTileMode::Offline,
                "Offline (vector)",
            );
            ui.separator();
            ui.weak(self.location_map.status_label());
        });

        ui.separator();

        let mut location_changed = false;
        let row_h = ui.available_height().clamp(240.0, 360.0);

        // Equal columns so the map always gets half the width (zone editor used to steal it).
        ui.columns(2, |cols| {
            cols[0].vertical(|ui| {
                ui.set_min_height(row_h);
                ui.label("Visibility zones (drag yellow corners; click a sector to select)");
                let zone_side = (ui.available_width() - 8.0)
                    .min(row_h - 40.0)
                    .clamp(160.0, 280.0);
                ui.add(ViewWindowEditor {
                    zones: self.view_windows,
                    state: self.zone_editor,
                    canvas_side: zone_side,
                });
            });

            cols[1].vertical(|ui| {
                ui.set_min_height(row_h);
                ui.label("Observer location (click map)");
                let map_h = (row_h - 28.0).max(180.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), map_h),
                    Layout::top_down(Align::Min),
                    |ui| {
                        location_changed = self.location_map.show(ui, self.long, self.lat);
                    },
                );
            });
        });

        location_changed
    }
}
