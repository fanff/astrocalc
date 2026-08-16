//! ISS opportunities panel: ~60-day list of visible passes and disk events.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use diesel::{Connection, SqliteConnection};
use egui::{Color32, Frame, Response, RichText, ScrollArea, Stroke, Ui};

use crate::config::ViewWindow;
use crate::models::IssEventRow;
use crate::panels::LatLon;
use crate::satellites::{
    DiskBody, DiskTransit, ISS_PREDICT_DAY_COUNT, PASS_STALE_AGE, TRANSIT_WARN_AGE, TleCache,
    VisiblePass, cloud_label, magnitude_phrase, moon_phase_label, naked_eye_label,
    utc_window_for_dates,
};
use crate::weather_cache::WeatherSnapshot;
use crate::widgets::iss_panel::{
    IssOpportunity, disk_move_hint, opportunity_list, pass_duration_secs, pass_elevation_phrase,
};

pub struct IssPanelState {
    pub lat_lon: LatLon,
    pub view_windows: Vec<ViewWindow>,
    pub bortle_class: u8,
    pub local_tz: Tz,
    /// Horizon start (UTC calendar date); predictions cover `[start, start+ISS_PREDICT_DAY_COUNT)`.
    pub start_date: NaiveDate,
    pub passes: Vec<VisiblePass>,
    pub sun_transits: Vec<DiskTransit>,
    pub moon_transits: Vec<DiskTransit>,
    pub tle_epoch: Option<DateTime<Utc>>,
    pub tle_fetched_at: Option<DateTime<Utc>>,
    pub weather_snapshot: Option<WeatherSnapshot>,
    pub weather_pending: bool,
    pub pending: bool,
    pub error: Option<String>,
    pub force_refresh: bool,
    pub request_predict: bool,
}

impl IssPanelState {
    pub fn new(lat_lon: LatLon, view_windows: Vec<ViewWindow>, bortle_class: u8) -> Self {
        Self {
            lat_lon,
            view_windows,
            bortle_class: bortle_class.clamp(1, 9),
            local_tz: Tz::UTC,
            start_date: Utc::now().date_naive(),
            passes: Vec::new(),
            sun_transits: Vec::new(),
            moon_transits: Vec::new(),
            tle_epoch: None,
            tle_fetched_at: None,
            weather_snapshot: None,
            weather_pending: false,
            pending: false,
            error: None,
            force_refresh: false,
            request_predict: true,
        }
    }

    pub fn set_local_tz(&mut self, tz: Tz) {
        self.local_tz = tz;
    }

    /// Load ISS events from SQLite for the current site/window. Returns true on cache hit.
    pub fn reload_cached_only(&mut self, database_url: &str, tle_cache: &TleCache) -> bool {
        let Ok(mut conn) = SqliteConnection::establish(database_url) else {
            return false;
        };
        let (start, end) = utc_window_for_dates(self.start_date, ISS_PREDICT_DAY_COUNT);
        match IssEventRow::try_load_bundle(
            &mut conn,
            self.lat_lon.lat,
            self.lat_lon.lon,
            start.timestamp_millis(),
            end.timestamp_millis(),
            tle_cache,
        ) {
            Ok(Some(bundle)) => {
                self.apply_bundle(
                    bundle.passes,
                    bundle.sun_transits,
                    bundle.moon_transits,
                    bundle.tle.tle_epoch,
                    bundle.tle.fetched_at,
                );
                true
            }
            _ => false,
        }
    }

    pub fn apply_bundle(
        &mut self,
        passes: Vec<VisiblePass>,
        sun_transits: Vec<DiskTransit>,
        moon_transits: Vec<DiskTransit>,
        tle_epoch: DateTime<Utc>,
        tle_fetched_at: DateTime<Utc>,
    ) {
        self.passes = passes;
        self.sun_transits = sun_transits;
        self.moon_transits = moon_transits;
        self.tle_epoch = Some(tle_epoch);
        self.tle_fetched_at = Some(tle_fetched_at);
        self.error = None;
        self.request_predict = false;
        self.force_refresh = false;
    }

    fn opportunities(&self) -> Vec<IssOpportunity<'_>> {
        opportunity_list(&self.passes, &self.sun_transits, &self.moon_transits)
    }

    fn cloud_at(&self, utc: DateTime<Utc>) -> Option<f64> {
        self.weather_snapshot
            .as_ref()
            .and_then(|s| s.cloud_cover_near(utc))
    }
}

fn fmt_utc_local(utc: DateTime<Utc>, local_tz: Tz, with_seconds: bool) -> (String, String) {
    let pat = if with_seconds {
        "%Y-%m-%d %H:%M:%S"
    } else {
        "%Y-%m-%d %H:%M"
    };
    let utc_s = format!("{} UTC", utc.format(pat));
    let local = utc.with_timezone(&local_tz);
    let local_s = format!("{} {}", local.format(pat), local.format("%Z"));
    (utc_s, local_s)
}

fn card_title_color(ui: &Ui) -> Color32 {
    ui.visuals().strong_text_color()
}

fn card_body_color(ui: &Ui) -> Color32 {
    // Brighter than default weak text on dark themes.
    if ui.visuals().dark_mode {
        Color32::from_rgb(210, 214, 220)
    } else {
        Color32::from_rgb(40, 44, 52)
    }
}

fn opportunity_card(ui: &mut Ui, line1: &str, line2: &str) {
    let title_c = card_title_color(ui);
    let body_c = card_body_color(ui);
    Frame::group(ui.style())
        .stroke(Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(line1).size(15.0).color(title_c).strong());
            ui.add_space(2.0);
            ui.label(RichText::new(line2).size(13.5).color(body_c));
        });
    ui.add_space(6.0);
}

impl egui::Widget for &mut IssPanelState {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        ui.horizontal(|ui| {
            ui.label(RichText::new("ISS opportunities").size(16.0).strong());
            ui.label(format!(
                "({ISS_PREDICT_DAY_COUNT} days from {})",
                self.start_date
            ));
            if self.pending {
                ui.spinner();
                ui.label("Computing passes / disk events...");
            } else if ui.button("Refresh orbit data").clicked() {
                self.force_refresh = true;
                self.request_predict = true;
                self.start_date = Utc::now().date_naive();
            }
        });

        ui.label(format!(
            "Site {:.4}, {:.4} - using Config view windows  |  Bortle {}",
            self.lat_lon.lat, self.lat_lon.lon, self.bortle_class
        ));
        ui.label(format!("Local timezone: {}", self.local_tz.name()));

        if let Some(err) = &self.error {
            ui.colored_label(Color32::RED, format!("ISS: {err}"));
        }

        if let Some(fetched) = self.tle_fetched_at {
            let age = Utc::now().signed_duration_since(fetched);
            let hours = age.num_minutes() as f64 / 60.0;
            ui.label(format!("TLE cache age: {hours:.1} h"));
            if age > PASS_STALE_AGE {
                ui.colored_label(
                    Color32::from_rgb(220, 120, 40),
                    "TLE older than 24 h - later pass times may drift; refresh recommended.",
                );
            } else if age > TRANSIT_WARN_AGE {
                ui.colored_label(
                    Color32::from_rgb(220, 120, 40),
                    "TLE older than 12 h - Sun/Moon disk corridor (~5-10 km) is uncertain.",
                );
            }
        }
        if let Some(epoch) = self.tle_epoch {
            let (utc_s, local_s) = fmt_utc_local(epoch, self.local_tz, false);
            ui.label(format!("TLE epoch: {utc_s} / {local_s}"));
        }

        ui.label(
            "Pass times use the sunlit + dark-sky window (not full geometric AOS-LOS). Magnitude is approximate (range + phase + airmass); no panel-flare model. Cloud labels use ~5-day forecast when available.",
        );
        ui.separator();

        let opps = self.opportunities();
        ui.label(
            RichText::new(format!("{} opportunities", opps.len()))
                .size(15.0)
                .strong(),
        );

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if opps.is_empty() && !self.pending && self.error.is_none() {
                    ui.label("None in this window for the current site / view zones.");
                }
                for opp in opps {
                    match opp {
                        IssOpportunity::Pass(p) => {
                            let (peak_utc, peak_local) =
                                fmt_utc_local(p.peak, self.local_tz, false);
                            let aos_u = p.aos.format("%H:%M");
                            let los_u = p.los.format("%H:%M");
                            let aos_l = p.aos.with_timezone(&self.local_tz).format("%H:%M");
                            let los_l = p.los.with_timezone(&self.local_tz).format("%H:%M");
                            let dur = pass_duration_secs(p);
                            let how = pass_elevation_phrase(p.max_altitude_deg);
                            let mag_ph = magnitude_phrase(p.peak_magnitude);
                            let quality = naked_eye_label(p.peak_magnitude, self.bortle_class);
                            let clouds = cloud_label(self.cloud_at(p.peak));
                            let line1 = format!(
                                "Visible pass - {how} ({:.0} deg) - {mag_ph} (mag {:.1})",
                                p.max_altitude_deg, p.peak_magnitude
                            );
                            let line2 = format!(
                                "Peak {peak_utc} / {peak_local}  |  visible {dur}s  |  AOS-LOS {aos_u}-{los_u} UTC ({aos_l}-{los_l} local)  |  phase {:.0} deg  |  {clouds}  |  {quality}",
                                p.phase_angle_deg
                            );
                            opportunity_card(ui, &line1, &line2);
                        }
                        IssOpportunity::Disk(e) => {
                            let body = match e.body {
                                DiskBody::Sun => "Sun",
                                DiskBody::Moon => "Moon",
                            };
                            let kind = if e.is_transit {
                                "transit"
                            } else {
                                "near-miss"
                            };
                            let (t_utc, t_local) =
                                fmt_utc_local(e.center_time, self.local_tz, true);
                            let phase = e
                                .moon_illum_pct
                                .map(|pct| format!("  |  {}", moon_phase_label(pct)))
                                .unwrap_or_default();
                            let move_txt = if e.is_transit {
                                "  |  on center-line".to_string()
                            } else {
                                disk_move_hint(e)
                                    .map(|s| format!("  |  {s}"))
                                    .unwrap_or_default()
                            };
                            let clouds = cloud_label(self.cloud_at(e.center_time));
                            let line1 = format!("{body} {kind}  {t_utc}  /  {t_local}");
                            let line2 = format!(
                                "sep {:.2} arcmin  |  disk R {:.2} arcmin  |  alt {:.0} deg  az {:.0} deg  |  {clouds}{phase}{move_txt}",
                                e.min_separation_deg * 60.0,
                                e.semi_diameter_deg * 60.0,
                                e.altitude_deg,
                                e.azimuth_deg,
                            );
                            opportunity_card(ui, &line1, &line2);
                        }
                    }
                }
            });

        ui.response()
    }
}
