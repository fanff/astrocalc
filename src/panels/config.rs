//! Config panel: named observer profiles, location map, and visibility zones.

use egui::{Align, Align2, Color32, ComboBox, Layout, Slider, Ui, Vec2, Window};

use crate::config::{AppSettings, ViewWindow};
use crate::models::ConfigProfile;
use crate::widgets::location_map::{LocationMap, MapTileMode};
use crate::widgets::view_window_editor::{ViewWindowEditor, ViewWindowEditorState};

#[derive(Debug)]
pub enum ConfigAction {
    Save,
    SaveAndSwitch(i32),
    SaveAs(String),
    Rename(String),
    Delete,
    Switch(i32),
}

#[derive(Default)]
pub struct ConfigPanelState {
    save_as_open: bool,
    rename_open: bool,
    delete_open: bool,
    pending_switch: Option<i32>,
    name_draft: String,
    pub status: Option<(String, bool)>,
}

pub struct ConfigPanelOutput {
    pub settings_changed: bool,
    pub action: Option<ConfigAction>,
}

/// Mutable inputs the Config panel edits.
pub struct ConfigPanel<'a> {
    pub lat: &'a mut f64,
    pub long: &'a mut f64,
    pub timezone_name: &'a str,
    pub view_windows: &'a mut Vec<ViewWindow>,
    pub bortle_class: &'a mut u8,
    pub zone_editor: &'a mut ViewWindowEditorState,
    pub location_map: &'a mut LocationMap,
    pub profiles: &'a [ConfigProfile],
    pub active_profile_id: i32,
    pub dirty: bool,
    pub state: &'a mut ConfigPanelState,
}

impl ConfigPanel<'_> {
    /// Draw the compact Config UI and return any persistence action requested by the user.
    pub fn show(mut self, ui: &mut Ui) -> ConfigPanelOutput {
        let before = self.current_settings();
        let mut action = None;
        if self.zone_editor.selected.is_none() && !self.view_windows.is_empty() {
            self.zone_editor.selected = Some(0);
        }

        ui.horizontal(|ui| {
            ui.strong("Configuration:");
            let active_name = self
                .profiles
                .iter()
                .find(|profile| profile.id == self.active_profile_id)
                .map(|profile| profile.name.as_str())
                .unwrap_or("Unknown");
            ComboBox::from_id_salt("config_profile_picker")
                .selected_text(if self.dirty {
                    format!("{active_name} •")
                } else {
                    active_name.to_string()
                })
                .show_ui(ui, |ui| {
                    for profile in self.profiles {
                        if ui
                            .selectable_label(profile.id == self.active_profile_id, &profile.name)
                            .clicked()
                            && profile.id != self.active_profile_id
                        {
                            if self.dirty {
                                self.state.pending_switch = Some(profile.id);
                            } else {
                                action = Some(ConfigAction::Switch(profile.id));
                            }
                        }
                    }
                });
            ui.separator();
            ui.label(format!("Location: {:.4}, {:.4}", *self.lat, *self.long));
            ui.separator();
            ui.label(format!("TZ: {}", self.timezone_name));
        });
        let can_save = self.current_settings().is_valid();
        if !can_save {
            ui.colored_label(
                Color32::YELLOW,
                "Save disabled: need a valid location and at least one valid visibility zone",
            );
        }

        ui.horizontal(|ui| {
            ui.label("Map source:");
            ui.radio_value(&mut self.location_map.mode, MapTileMode::Auto, "Auto");
            ui.radio_value(
                &mut self.location_map.mode,
                MapTileMode::Online,
                "OSM (online)",
            );
            ui.radio_value(
                &mut self.location_map.mode,
                MapTileMode::Offline,
                "Offline (no tiles)",
            );
            ui.separator();
            ui.weak(self.location_map.status_label());
        });

        ui.horizontal(|ui| {
            ui.label("Sky brightness (Bortle):");
            ui.add(Slider::new(self.bortle_class, 1..=9).step_by(1.0));
            ui.weak(bortle_hint(*self.bortle_class));
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
                    zones: &mut *self.view_windows,
                    state: &mut *self.zone_editor,
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
                        location_changed = self.location_map.show(
                            ui,
                            self.long,
                            self.lat,
                            self.view_windows,
                            self.zone_editor.selected,
                        );
                    },
                );
            });
        });

        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_save && self.dirty, egui::Button::new("Save"))
                .on_hover_text("Update the selected configuration")
                .clicked()
            {
                action = Some(ConfigAction::Save);
            }
            if ui
                .add_enabled(can_save, egui::Button::new("Save as…"))
                .clicked()
            {
                self.state.name_draft.clear();
                self.state.save_as_open = true;
            }
            if ui.button("Rename…").clicked() {
                self.state.name_draft = self
                    .profiles
                    .iter()
                    .find(|profile| profile.id == self.active_profile_id)
                    .map(|profile| profile.name.clone())
                    .unwrap_or_default();
                self.state.rename_open = true;
            }
            if ui
                .add_enabled(self.profiles.len() > 1, egui::Button::new("Delete…"))
                .clicked()
            {
                self.state.delete_open = true;
            }
            if self.dirty {
                ui.weak("Unsaved changes");
            }
        });

        if let Some((message, success)) = &self.state.status {
            ui.colored_label(
                if *success {
                    Color32::from_rgb(100, 210, 130)
                } else {
                    Color32::from_rgb(255, 120, 100)
                },
                message,
            );
        }

        self.show_name_dialog(ui, &mut action, true);
        self.show_name_dialog(ui, &mut action, false);
        self.show_delete_dialog(ui, &mut action);
        self.show_switch_dialog(ui, can_save, &mut action);

        ConfigPanelOutput {
            settings_changed: location_changed || before != self.current_settings(),
            action,
        }
    }

    fn current_settings(&self) -> AppSettings {
        AppSettings {
            lat: *self.lat,
            lon: *self.long,
            view_windows: self.view_windows.clone(),
            bortle_class: *self.bortle_class,
        }
    }

    fn show_name_dialog(&mut self, ui: &Ui, action: &mut Option<ConfigAction>, save_as: bool) {
        let is_open = if save_as {
            self.state.save_as_open
        } else {
            self.state.rename_open
        };
        if !is_open {
            return;
        }
        let title = if save_as {
            "Save configuration as"
        } else {
            "Rename configuration"
        };
        let mut open = true;
        let mut close = false;
        Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label("Name");
                let response = ui.text_edit_singleline(&mut self.state.name_draft);
                response.request_focus();
                let valid = !self.state.name_draft.trim().is_empty();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            valid,
                            egui::Button::new(if save_as { "Create" } else { "Rename" }),
                        )
                        .clicked()
                    {
                        let name = self.state.name_draft.trim().to_string();
                        *action = Some(if save_as {
                            ConfigAction::SaveAs(name)
                        } else {
                            ConfigAction::Rename(name)
                        });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        let remains_open = open && !close;
        if save_as {
            self.state.save_as_open = remains_open;
        } else {
            self.state.rename_open = remains_open;
        }
    }

    fn show_delete_dialog(&mut self, ui: &Ui, action: &mut Option<ConfigAction>) {
        if !self.state.delete_open {
            return;
        }
        let mut open = true;
        let mut close = false;
        Window::new("Delete configuration")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label("Delete this saved configuration? This cannot be undone.");
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        *action = Some(ConfigAction::Delete);
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        self.state.delete_open = open && !close;
    }

    fn show_switch_dialog(&mut self, ui: &Ui, can_save: bool, action: &mut Option<ConfigAction>) {
        let Some(target) = self.state.pending_switch else {
            return;
        };
        let target_name = self
            .profiles
            .iter()
            .find(|profile| profile.id == target)
            .map(|profile| profile.name.as_str())
            .unwrap_or("selected configuration");
        Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.label(format!("Save changes before switching to “{target_name}”?"));
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_save, egui::Button::new("Save and switch"))
                        .clicked()
                    {
                        *action = Some(ConfigAction::SaveAndSwitch(target));
                        self.state.pending_switch = None;
                    }
                    if ui.button("Discard and switch").clicked() {
                        *action = Some(ConfigAction::Switch(target));
                        self.state.pending_switch = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.state.pending_switch = None;
                    }
                });
            });
    }
}

fn bortle_hint(class: u8) -> &'static str {
    match class {
        1 => "excellent dark sky",
        2 => "typical truly dark",
        3 => "rural sky",
        4 => "rural/suburban",
        5 => "suburban",
        6 => "bright suburban",
        7 => "suburban/urban",
        8 => "city sky",
        _ => "inner city",
    }
}
