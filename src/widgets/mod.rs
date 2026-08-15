use egui::Response;
pub mod calendar_plot;
pub mod catalog_select;
pub mod iss_panel;
pub mod location_map;
pub mod sky_map;
pub mod sky_polar;
pub mod vector_basemap;
pub mod view_window_editor;
pub mod weather;

pub use catalog_select::CatalogSelection;

#[derive(Clone)]
pub struct ObjectSelectedFlags {
    pub saturn: bool,
    pub jupiter: bool,
    pub neptune: bool,
    pub uranus: bool,
    pub mercury: bool,
    pub venus: bool,
    pub mars: bool,
    pub moon: bool,
}
impl Default for ObjectSelectedFlags {
    fn default() -> Self {
        Self {
            saturn: true,
            jupiter: true,
            neptune: false,
            uranus: false,
            mercury: false,
            venus: true,
            mars: true,
            moon: true,
        }
    }
}
impl ObjectSelectedFlags {
    pub fn selected_object_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for name in ObjectSelectedFlags::all_names() {
            if let Some(flag) = self.get_bool_flag(&name) {
                if *flag {
                    names.push(name);
                }
            }
        }
        names
    }
    pub fn all_names() -> Vec<String> {
        vec![
            "Saturn".into(),
            "Jupiter".into(),
            "Neptune".into(),
            "Uranus".into(),
            "Mercury".into(),
            "Venus".into(),
            "Mars".into(),
            "Moon".into(),
        ]
    }

    pub fn get_bool_flag(&self, name: &str) -> Option<&bool> {
        match name {
            "Saturn" => Some(&self.saturn),
            "Moon" => Some(&self.moon),
            "Jupiter" => Some(&self.jupiter),
            "Neptune" => Some(&self.neptune),
            "Uranus" => Some(&self.uranus),
            "Mercury" => Some(&self.mercury),
            "Venus" => Some(&self.venus),
            "Mars" => Some(&self.mars),
            _ => None,
        }
    }
    pub fn get_mut_bool_flag(&mut self, name: &str) -> Option<&mut bool> {
        match name {
            "Saturn" => Some(&mut self.saturn),
            "Moon" => Some(&mut self.moon),
            "Jupiter" => Some(&mut self.jupiter),
            "Neptune" => Some(&mut self.neptune),
            "Uranus" => Some(&mut self.uranus),
            "Mercury" => Some(&mut self.mercury),
            "Venus" => Some(&mut self.venus),
            "Mars" => Some(&mut self.mars),
            _ => None,
        }
    }
}

impl egui::Widget for &mut ObjectSelectedFlags {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        ui.horizontal(|ui| {
            for name in ObjectSelectedFlags::all_names() {
                ui.checkbox(self.get_mut_bool_flag(&name).unwrap(), &name);
            }
        });
        ui.response()
    }
}
