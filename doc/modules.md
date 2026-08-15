# Modules

Ownership map for the single binary crate. When adding code, put it in the module that owns the concern — or add a row here first.

## Bootstrap

| Module | Owns | Do not |
|--------|------|--------|
| [`src/main.rs`](../src/main.rs) | Process entry: dotenv, migrations, load/seed settings, weather cache init, eframe launch | Business logic, Diesel queries, panel layout |

## UI layer

| Module | Owns | Do not |
|--------|------|--------|
| [`src/gui.rs`](../src/gui.rs) | `AstroCalcApp` shell, panel routing, shared session state (lat/lon, view windows, binds) | Deep ephemeris math; keep growing god-object fields in check |
| [`src/panels/`](../src/panels/) | Screen-level UI: `LatLon`, Config, Daily, Long Term | Reusable plot primitives (those go in widgets) |
| [`src/panels/config.rs`](../src/panels/config.rs) | Config layout: location map, visibility zones, Save → SQLite | Schema definitions; GeoJSON parsing |
| [`src/panels/dailysolar.rs`](../src/panels/dailysolar.rs) | Daily night exploration: load cached day, request background prefetch (day+10), object flags, weather panel, compose sky + calendar plots | Weather HTTP; ISS; raw Diesel schema details if a repository helper exists |
| [`src/panels/iss.rs`](../src/panels/iss.rs) | ISS opportunities view: ~60-day chronological list of sunlit passes + Sun/Moon disk events | TLE HTTP / SGP4 (lives in `satellites`) |
| [`src/panels/longterm_plot.rs`](../src/panels/longterm_plot.rs) | Multi-night presence overview from DB (≥20 min in view; catalog selection; zoomable) | Duplicate daily filtering logic — share domain filters |
| [`src/widgets/`](../src/widgets/) | Reusable controls/plots: sky map, polar helpers, view-window zone editor, location map, calendar plot, weather panel, ISS opportunity helpers, catalog selection | Opening DB connections; owning app-wide config |
| [`src/widgets/location_map.rs`](../src/widgets/location_map.rs) | OSM `HttpTiles` + offline vector basemap, click marker, online probe / mode | Config Save; view-window editing |
| [`src/widgets/vector_basemap.rs`](../src/widgets/vector_basemap.rs) | Parse/paint embedded GeoJSON (countries, FR regions, places) via walkers `Plugin` | HTTP tiles; config persistence |
| [`src/widgets/iss_panel.rs`](../src/widgets/iss_panel.rs) | Merge/sort ISS opportunities for the ISS panel list | TLE fetch; SGP4 |

## Domain layer

| Module | Owns | Do not |
|--------|------|--------|
| [`src/config.rs`](../src/config.rs) | `AppSettings`, `ViewWindow`, validation, Paris defaults | Ephemeris; UI layout; Diesel I/O |
| [`src/solarsystemcalc.rs`](../src/solarsystemcalc.rs) | Planet/Moon (and Sun) ephemeris, night intervals, sampling, `ObjectPosition` encode/decode, segment helpers | egui; weather; DSO catalog parsing |
| [`src/deepsky/`](../src/deepsky/) | Embedded catalog load, magnitude filter, DSO position calculation, `ensure_dso_positions` cache merge | Solar-system planet formulas; UI |
| [`src/deepsky/data.rs`](../src/deepsky/data.rs) | `DeepObject`, `DeepSkyCatalog`, `CATALOG` | Alt-az conversion (belongs beside calc in `deepsky/mod.rs` or shared coords) |
| [`src/satellites/`](../src/satellites/) | ISS TLE (Celestrak) cache, SGP4 propagate, sunlit passes (visible window + mag/phase), Sun/Moon disk transit/near-miss | Photo overlay; general satellite catalog |
| *future `visibility`* | Compose view windows, mag limit, FOV, weather thresholds | Plotting |
| *future `conjunctions`* | ISS vs planet/DSO angular events | Photo overlay rendering |

## Infra layer

| Module | Owns | Do not |
|--------|------|--------|
| [`src/models.rs`](../src/models.rs) | Diesel row types and DB load/insert helpers for settings, nights, position chunks | egui |
| [`src/schema.rs`](../src/schema.rs) | Diesel `table!` definitions (generated) | Hand-edit casually — prefer migrations |
| [`src/weather_cache.rs`](../src/weather_cache.rs) | Snap location, YAML cache files, Open-Meteo fetch, `WeatherSnapshot` parsing | Drawing forecast charts (UI consumes structured data) |
| `migrations/` | Schema evolution | Application logic |

## Data and assets (not Rust modules)

| Path | Role |
|------|------|
| `src/deepsky/ngc-ic-messier-catalog.csv` | Embedded DSO catalog |
| `database.db` | Local SQLite (settings + ephemeris cache) |
| `assets/vector_map/` | Offline vector basemap GeoJSON (countries, France regions, places) |
| `assets/icon.svg` / `assets/icon.png` | App icon (SVG source; PNG embedded for the window) |
| `my_weather_app/` | Weather YAML cache directory |
| `my_tle_cache/` | ISS TLE JSON cache (`iss_tle.json`) |
| `background/` | Location photos for future overlay |

## Dependency direction

```
main → gui → panels / widgets
           → config / weather_cache / models   (infra)
           → solarsystemcalc / deepsky         (domain)

panels → widgets
panels → models / solarsystemcalc / config

models → schema, config (`AppSettings`), solarsystemcalc types (NightInfo, ObjectPosition)
```

Avoid `widgets → models` and `solarsystemcalc → gui`.

## When to split further

| Trigger | Action |
|---------|--------|
| DSO + ISS share segment/filter types | Extract shared `positions` / `nights` from `solarsystemcalc` |
| Multiple panels open DB the same way | Thin repository API in infra; panels call that |
| Hardware preview + overlay land | `ui/overlay` + domain `hardware` / plate-scale; no second ephemeris |
| Crate compile times or reuse from CLI | Introduce `lib.rs` exporting domain + infra |

## Checklist for a new feature module

1. Name the owner layer (ui / domain / infra).
2. Add or update a row in this file.
3. Extend [data-model.md](data-model.md) if persistence or config changes.
4. Prefer extending existing position/filter pipelines over new parallel formats.
