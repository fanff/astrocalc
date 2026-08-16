use csv::Trim;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::io::Cursor;

#[derive(Debug, Clone, Deserialize)]
pub struct DeepObject {
    #[serde(rename = "IC - NGC")]
    pub ic_ngc: Option<String>,
    #[serde(rename = "Name")]
    pub name: Option<String>,
    #[serde(rename = "Messier")]
    pub messier: Option<String>,
    #[serde(rename = "NGC")]
    pub ngc: Option<String>,
    #[serde(rename = "IC")]
    pub ic: Option<String>,
    #[serde(rename = "Object Type abrev.")]
    pub object_type_abrev: Option<String>,
    #[serde(rename = "Object type")]
    pub object_type: Option<String>,
    #[serde(rename = "ra")]
    pub ra: Option<String>,
    #[serde(rename = "dec")]
    pub dec: Option<String>,
    #[serde(rename = "Constellation")]
    pub constellation: Option<String>,
    #[serde(rename = "Major axis")]
    pub major_axis: Option<f64>,
    #[serde(rename = "Minor axis")]
    pub minor_axis: Option<f64>,
    #[serde(rename = "Position angle")]
    pub position_angle: Option<f64>,
    #[serde(rename = "b_mag")]
    pub b_mag: Option<f32>,
    #[serde(rename = "v_mag")]
    pub v_mag: Option<f32>,
    #[serde(rename = "j_mag")]
    pub j_mag: Option<f32>,
    #[serde(rename = "h_mag")]
    pub h_mag: Option<f32>,
    #[serde(rename = "k_mag")]
    pub k_mag: Option<f32>,
    #[serde(rename = "Surface Brigthness")]
    pub surface_brigthness: Option<f32>, // note: header is spelled "Brigthness"
    #[serde(rename = "Hubble (only Galaxies)")]
    pub hubble_only_galaxies: Option<String>,
    #[serde(rename = "Cstar U-Mag (only Planetary Nebulae)")]
    pub cstar_u_mag_only_pn: Option<f32>,
    #[serde(rename = "Cstar B-Mag (only Planetary Nebulae)")]
    pub cstar_b_mag_only_pn: Option<f32>,
    #[serde(rename = "Cstar V-Mag (only Planetary Nebulae)")]
    pub cstar_v_mag_only_pn: Option<f32>,
    #[serde(rename = "Cstar Names (only Planetary Nebulae)")]
    pub cstar_names_only_pn: Option<String>,
    #[serde(rename = "identifiers")]
    pub identifiers: Option<String>,
    #[serde(rename = "common_names")]
    pub common_names: Option<String>,
    #[serde(rename = "ned_notes")]
    pub ned_notes: Option<String>,
    #[serde(rename = "openngc_notes")]
    pub openngc_notes: Option<String>,
    #[serde(rename = "Image")]
    image: Option<String>,
}

impl DeepObject {
    /// Stable UI / segment id: prefer Messier (`M31`), else `NGC…`, else catalog `Name`.
    pub fn display_id(&self) -> Option<String> {
        if let Some(m) = self
            .messier
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let id = if m.starts_with('M') || m.starts_with('m') {
                m.to_uppercase()
            } else {
                format!("M{m}")
            };
            return Some(id);
        }
        if let Some(n) = self
            .ngc
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let id = if n.to_uppercase().starts_with("NGC") {
                n.to_uppercase().replace(' ', "")
            } else {
                format!("NGC{n}")
            };
            return Some(id);
        }
        self.name
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn type_label(&self) -> String {
        self.object_type
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.object_type_abrev
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "DSO".into())
    }

    pub fn has_messier(&self) -> bool {
        self.messier
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    pub fn has_ngc(&self) -> bool {
        self.ngc
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    pub fn matches_search(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let hay: Vec<String> = [
            self.display_id(),
            self.messier.clone(),
            self.ngc.clone(),
            self.name.clone(),
            self.common_names.clone(),
            self.object_type.clone(),
        ]
        .into_iter()
        .flatten()
        .collect();
        hay.iter().any(|s| s.to_lowercase().contains(&q))
    }

    /// Parse catalog RA (`HH:MM:SS.ss`) and Dec (`±DD:MM:SS.s`) to radians.
    pub fn ra_dec_rad(&self) -> Option<(f64, f64)> {
        let ra_str = self.ra.as_ref()?;
        let dec_str = self.dec.as_ref()?;
        let ra_deg = parse_hms_to_degrees(ra_str)?;
        let dec_deg = parse_dms_to_degrees(dec_str)?;
        Some((ra_deg.to_radians(), dec_deg.to_radians()))
    }
}

/// Catalog RA is hours:minutes:seconds → degrees via `astro::angle::deg_frm_hms`.
pub fn parse_hms_to_degrees(s: &str) -> Option<f64> {
    let parts = split_sexagesimal(s)?;
    if parts.len() < 2 {
        return None;
    }
    let h = parts[0] as i64;
    let m = parts[1].abs() as i64;
    let sec = if parts.len() > 2 { parts[2].abs() } else { 0.0 };
    Some(astro::angle::deg_frm_hms(h, m, sec))
}

/// Catalog Dec is signed degrees:arcmin:arcsec.
pub fn parse_dms_to_degrees(s: &str) -> Option<f64> {
    let parts = split_sexagesimal(s)?;
    if parts.len() < 2 {
        return None;
    }
    let deg = parts[0] as i64;
    let min = parts[1].abs() as i64;
    let sec = if parts.len() > 2 { parts[2].abs() } else { 0.0 };
    Some(astro::angle::deg_frm_dms(deg, min, sec))
}

fn split_sexagesimal(s: &str) -> Option<Vec<f64>> {
    let cleaned = s.trim().replace(' ', "");
    if cleaned.is_empty() {
        return None;
    }
    let parts: Result<Vec<f64>, _> = cleaned.split(':').map(|p| p.parse::<f64>()).collect();
    parts.ok().filter(|v| !v.is_empty())
}

// Embed the CSV bytes (or use include_str! if it's guaranteed UTF-8)
static CSV_TEXT: &str = include_str!("ngc-ic-messier-catalog.csv");

#[derive(Debug, Clone)]
pub struct DeepSkyCatalog {
    pub objects: Vec<DeepObject>,
}
impl DeepSkyCatalog {
    pub fn len(&self) -> usize {
        self.objects.len()
    }
    pub fn filter_magnitude(&self, max_v_mag: f32) -> DeepSkyCatalog {
        let filter: Vec<&DeepObject> = self
            .objects
            .iter()
            .filter(|obj| match obj.v_mag {
                Some(mag) => mag <= max_v_mag,
                None => false,
            })
            .collect();
        let vec_filtered: Vec<DeepObject> = filter.iter().map(|&obj| obj.clone()).collect();
        DeepSkyCatalog {
            objects: vec_filtered,
        }
    }

    pub fn messier_objects(&self) -> Vec<&DeepObject> {
        let mut out: Vec<&DeepObject> = self.objects.iter().filter(|o| o.has_messier()).collect();
        out.sort_by(|a, b| {
            mag_key(a.v_mag)
                .partial_cmp(&mag_key(b.v_mag))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.display_id().cmp(&b.display_id()))
        });
        out
    }

    /// NGC objects that do not also have a Messier id (avoid double listing).
    pub fn ngc_without_messier(&self) -> Vec<&DeepObject> {
        let mut out: Vec<&DeepObject> = self
            .objects
            .iter()
            .filter(|o| o.has_ngc() && !o.has_messier())
            .collect();
        out.sort_by(|a, b| {
            mag_key(a.v_mag)
                .partial_cmp(&mag_key(b.v_mag))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.display_id().cmp(&b.display_id()))
        });
        out
    }

    pub fn find_by_display_id(&self, id: &str) -> Option<&DeepObject> {
        let needle = id.trim().to_uppercase().replace(' ', "");
        self.objects.iter().find(|o| {
            o.display_id()
                .map(|d| d.to_uppercase().replace(' ', "") == needle)
                .unwrap_or(false)
        })
    }
}

fn mag_key(v: Option<f32>) -> f32 {
    v.unwrap_or(f32::INFINITY)
}

// Parse once on first access
pub static CATALOG: Lazy<DeepSkyCatalog> = Lazy::new(|| {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(b';') // your file uses semicolons
        .has_headers(true)
        .trim(Trim::All) // trim spaces around fields
        .from_reader(Cursor::new(CSV_TEXT));

    DeepSkyCatalog {
        objects: rdr
            .deserialize()
            .collect::<Result<Vec<DeepObject>, _>>()
            .expect("invalid CSV"),
    }
});
