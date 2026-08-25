pub mod config;

use crate::gui::AstroCalcApp;
use crate::models::ConfigProfile;
use crate::weather_cache::WeatherCache;
use diesel::{Connection, SqliteConnection};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use dotenvy::dotenv;
use eframe::egui;
use std::env;

mod deepsky;
mod gui;
pub mod models;
mod panels;
mod satellites;
pub mod schema;
mod solarsystemcalc;
mod timezone_util;
mod weather_cache;
mod widgets;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

fn resolve_database_url() -> String {
    env::var("DATABASE_URL").unwrap_or_else(|_| "database.db".into())
}

fn open_and_migrate(database_url: &str) -> Result<SqliteConnection, Box<dyn std::error::Error>> {
    let mut conn = SqliteConnection::establish(database_url)?;
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| format!("migration failed: {e}"))?;
    Ok(conn)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let database_url = resolve_database_url();
    let mut conn = open_and_migrate(&database_url)?;
    let active_profile = ConfigProfile::load_or_seed_active(&mut conn)?;
    drop(conn);

    let weather_cache = WeatherCache::new(
        "my_weather_app",
        chrono::Duration::minutes(30), // freshness: 30 minutes
        2,                             // coord precision: ~1 km
    )?;
    let tle_cache =
        crate::satellites::TleCache::new("my_tle_cache", crate::satellites::DEFAULT_TLE_FRESHNESS)?;

    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AstroCalc")
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "AstroCalc",
        options,
        Box::new(move |cc| {
            Ok(Box::new(AstroCalcApp::new(
                cc.egui_ctx.clone(),
                active_profile,
                database_url,
                weather_cache,
                tle_cache,
            )))
        }),
    )
    .unwrap();
    Ok(())
}

#[cfg(test)]
mod boot_tests {
    use super::*;
    use crate::models::ConfigProfile;

    #[test]
    fn migrate_and_seed_temp_database() {
        let path = std::env::temp_dir().join("astrocalc_boot_settings_test.db");
        let _ = std::fs::remove_file(&path);
        let url = path.to_str().unwrap().to_string();
        let mut conn = open_and_migrate(&url).expect("migrate");
        let profile = ConfigProfile::load_or_seed_active(&mut conn).expect("seed");
        assert_eq!(profile.name, "Default");
        assert!(profile.settings.is_valid());
        assert!((profile.settings.lat - 48.8566).abs() < 1e-9);
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }
}
