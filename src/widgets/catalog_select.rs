use std::collections::{HashMap, HashSet};

use egui::{Response, RichText, ScrollArea};

use crate::deepsky::data::{CATALOG, DeepObject};
use crate::widgets::ObjectSelectedFlags;

/// Daily object picker: planets always visible; Messier/NGC behind collapsible headers.
#[derive(Clone)]
pub struct CatalogSelection {
    pub planets: ObjectSelectedFlags,
    pub messier_search: String,
    pub ngc_search: String,
    pub messier_mag_limit: f32,
    pub ngc_mag_limit: f32,
    /// Selected deep-sky display ids (`M31`, `NGC7000`, …).
    pub selected_dso: HashSet<String>,
}

impl Default for CatalogSelection {
    fn default() -> Self {
        Self {
            planets: ObjectSelectedFlags::default(),
            messier_search: String::new(),
            ngc_search: String::new(),
            messier_mag_limit: 9.0,
            ngc_mag_limit: 10.0,
            selected_dso: HashSet::new(),
        }
    }
}

impl CatalogSelection {
    pub fn selected_planet_names(&self) -> Vec<String> {
        self.planets.selected_object_names()
    }

    pub fn selected_dso_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.selected_dso.iter().cloned().collect();
        ids.sort();
        ids
    }

    /// All names used by `filter_view` / plot pipelines.
    pub fn selected_object_names(&self) -> Vec<String> {
        let mut names = self.selected_planet_names();
        names.extend(self.selected_dso_ids());
        names
    }

    pub fn planet_type_map() -> HashMap<String, String> {
        let mut m = HashMap::new();
        for name in ObjectSelectedFlags::all_names() {
            let label = if name == "Moon" {
                "Moon".to_string()
            } else {
                "Planet".to_string()
            };
            m.insert(name, label);
        }
        m
    }
}

impl egui::Widget for &mut CatalogSelection {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        ui.horizontal_wrapped(|ui| {
            for name in ObjectSelectedFlags::all_names() {
                ui.checkbox(self.planets.get_mut_bool_flag(&name).unwrap(), &name);
            }
        });

        ui.collapsing("Messier", |ui| {
            ui.horizontal(|ui| {
                ui.label("search");
                ui.text_edit_singleline(&mut self.messier_search);
                ui.label("mag ≤");
                ui.add(
                    egui::Slider::new(&mut self.messier_mag_limit, 4.0..=12.0)
                        .step_by(0.5)
                        .fixed_decimals(1),
                );
            });
            dso_checkbox_list(
                ui,
                CATALOG.messier_objects(),
                &self.messier_search,
                self.messier_mag_limit,
                &mut self.selected_dso,
            );
        });

        ui.collapsing("NGC", |ui| {
            ui.horizontal(|ui| {
                ui.label("search");
                ui.text_edit_singleline(&mut self.ngc_search);
                ui.label("mag ≤");
                ui.add(
                    egui::Slider::new(&mut self.ngc_mag_limit, 4.0..=14.0)
                        .step_by(0.5)
                        .fixed_decimals(1),
                );
            });
            dso_checkbox_list(
                ui,
                CATALOG.ngc_without_messier(),
                &self.ngc_search,
                self.ngc_mag_limit,
                &mut self.selected_dso,
            );
        });

        ui.response()
    }
}

fn dso_checkbox_list(
    ui: &mut egui::Ui,
    objects: Vec<&DeepObject>,
    search: &str,
    mag_limit: f32,
    selected: &mut HashSet<String>,
) {
    let filtered: Vec<&DeepObject> = objects
        .into_iter()
        .filter(|o| o.matches_search(search))
        .filter(|o| match o.v_mag {
            Some(m) => m <= mag_limit,
            None => false,
        })
        .collect();

    ui.label(
        RichText::new(format!("{} objects", filtered.len()))
            .small()
            .weak(),
    );

    ScrollArea::vertical()
        .max_height(140.0)
        .show(ui, |ui| {
            for obj in filtered {
                let Some(id) = obj.display_id() else {
                    continue;
                };
                let mut on = selected.contains(&id);
                let mag = obj
                    .v_mag
                    .map(|m| format!("{m:.1}"))
                    .unwrap_or_else(|| "—".into());
                let label = format!("{id}  mag {mag}  ·  {}", obj.type_label());
                if ui.checkbox(&mut on, label).changed() {
                    if on {
                        selected.insert(id);
                    } else {
                        selected.remove(&id);
                    }
                }
            }
        });
}
