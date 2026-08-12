# Architecture

This document is the stability contract for AstroCalc. New features should fit these boundaries so the desktop app stays responsive and the codebase does not accumulate parallel half-systems.

## Product shape

- **Primary deliverable:** a native desktop application (egui/eframe).
- **Primary language:** Rust — for ephemeris throughput and UI responsiveness under multi-day sampling.
- **Core loop:** configure observer + view → precompute object positions for nights → filter by visibility constraints (geometry, weather, magnitude, FOV) → visualize (plots, later photo overlay / frame preview).

## Layering

Keep three layers. Cross-layer imports should only go “downward.”

```
ui          gui, panels, widgets
domain      ephemeris, nights, visibility filters, catalogs, object kinds
infra       Diesel/SQLite, weather HTTP + YAML cache, asset paths
```

| Layer | May depend on | Must not |
|-------|---------------|----------|
| `ui` | domain types, infra facades for load/save | embed Diesel queries or HTTP in leaf widgets long-term |
| `domain` | pure math/time libs, catalog data | egui, Diesel connections, filesystem paths |
| `infra` | domain serializable types | egui widgets |

Today the crate is a single binary (`src/main.rs` only). That is acceptable for now. When domain logic grows (DSO cache, ISS, hardware model), extract a `lib.rs` (or a small workspace crate) **before** adding a second UI surface — do not invent a multi-crate workspace without a concrete boundary need.

## Cache-first ephemeris

Expensive alt-az sampling is **precomputed** and stored. The UI reads the cache; recalculation happens on miss or when the cache key is invalidated.

Current solar-system path:

1. Build night intervals for the date range and location.
2. Sample object positions at a chosen frequency during each night.
3. Persist night metadata (`dateinfo`) and a bincode blob of positions (`objectposition`).
4. Daily (and later long-term) panels load from SQLite instead of recomputing.

**Rules:**

- One canonical on-disk position format per object family; version or migrate blobs instead of inventing a second blob schema per panel.
- Skip days already present for the same geo sector unless the user forces recalculation.
- Sampling policy (frequency, night definition) is part of the cache contract — changing it requires invalidation or a new key dimension.

## Geo sectoring

Lat/lon are snapped (typically 2 decimal places, ~1 km) for:

- SQLite rows (`lat_sector` / `lon_sector`)
- Weather YAML cache keys

This is intentional approximation: nearby observers share cache entries. Document any change to precision as a breaking cache change.

## View windows and visibility

`ViewWindow` (az/alt rectangles in config) is a first-class filter. Visibility filtering must remain a **pure domain function** over positions + constraints, not logic duplicated inside each plot.

Future constraint types (limit magnitude, FOV, cloud cover thresholds, hardware reach) should compose with the same filter pipeline:

```
positions → geometric view windows → magnitude / FOV → weather quality → UI
```

## Object kinds

Grow a closed set of object families carefully. Prefer shared time/position/segment types over one mega-struct with every field optional.

| Kind | Examples | Notes |
|------|----------|--------|
| SolarSystem | Sun, Moon, planets | Implemented path today (Sun incomplete) |
| DeepSky | NGC/IC/Messier | Catalog present; pipeline incomplete |
| Satellite | ISS | Roadmap |
| Conjunction | ISS vs body / body vs body | Roadmap; derived events, not raw tracks alone |

Presentation features (photo overlay, expected frame preview) consume the same domain positions; they must not define a separate ephemeris path.

## Async and responsiveness

- Multi-day sampling and network weather calls must not block the egui frame loop.
- Use `egui-async::Bind` (or the same pattern) for long jobs; show progress/errors in the panel that requested the work.
- Prefer streaming or day-batched writes to SQLite for large ranges rather than holding unbounded vectors only in UI state.

## Configuration surface

User-facing durable settings (observer lat/lon, view windows) live in SQLite (`app_settings`). Connection path and secrets stay in env (`.env` → `DATABASE_URL`).

Extend settings for FOV, limit magnitude, hardware profile, overlay image paths — **do not hardcode** these in panels. Panel state may hold ephemeral UI values; persistence goes through the settings row or other DB tables.

## Persistence choices

| Store | Role |
|-------|------|
| SQLite + Diesel | App settings (`app_settings`); night spans + position blobs; query by date × sector |
| YAML weather files | Forecast cache under `my_weather_app/` |
| Embedded CSV | Deep-sky catalog (`include_str!`) |

Do not add a second ORM or a second UI framework. Prefer extending Diesel migrations and the existing bincode position pipeline.

## Anti-bloat rules

1. No new top-level module without an owner row in [modules.md](modules.md).
2. No parallel “quick” position formats for one panel — extend shared types.
3. Widgets draw; panels orchestrate; domain computes; infra persists.
4. Photo overlay and hardware preview are presentation over domain positions.
5. Feature flags in the UI (object toggles) must map to domain filters, not ad-hoc branches scattered across plots.
6. Delete or quarantine unused dependencies when a feature lands; do not leave stub crates “for later” without a roadmap entry.

## Evolution path (when needed)

Reasonable next structural steps (triggered by feature work, not beforehand):

1. `lib.rs` exporting domain + infra; `main` only bootstraps eframe.
2. Split `solarsystemcalc` vs shared `positions` / `nights` modules when DSO/ISS share segment types.
3. Blob versioning or separate tables if DSO/ISS payloads diverge from solar-system `ObjectPosition`.
4. Dedicated `visibility` module composing windows, magnitude, weather, FOV.
