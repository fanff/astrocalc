# Roadmap

Prioritized backlog for AstroCalc. Order is intentional: finish data that already exists in the crate, then unify filters.

## Priority features

### 1. Weather in the UI

Integrate the existing Open-Meteo + YAML cache into the desktop UI cleanly.

- [x] Fetch/display forecast for the selected location (and date/night context) — Daily panel
- [x] Surface **cloud coverage**, **humidity**, and **wind** (speed/direction as available from the API)
- [x] Show cache freshness and geo-sector so users know when data was snapped/fetched
- [ ] Use weather as an input to planning (at least visible; later as a visibility/quality filter)

**Depends on:** [src/weather_cache.rs](../src/weather_cache.rs) (`WeatherSnapshot`), Daily widget [src/widgets/weather.rs](../src/widgets/weather.rs), async Bind in `AstroCalcApp`.

### 2. Finish Sun / Moon / planets

Close gaps in the solar-system family.

- Complete Sun handling alongside planets and Moon
- Verify magnitudes, phase (Moon), and night/day edge cases
- [x] ~~**Long Term** panel~~ removed — presence overview superseded by Night Tracks
- [x] **Night Tracks** panel: multi-night timeline (night hours × dates; twilight clear-sky background; altitude-encoded object segments; no weather)
- [x] Solar System calc panel removed — Daily prefetches selected night + 10 days in background
- Keep Daily and Night Tracks on the same position types and filters

**Depends on:** [src/solarsystemcalc.rs](../src/solarsystemcalc.rs), [src/panels/night_tracks.rs](../src/panels/night_tracks.rs).

### 3. FOV, limit magnitude, visibility filtering

Settings-driven constraints applied uniformly across object kinds.

- Persist FOV and limit magnitude in `app_settings` (not panel-only state)
- Filter positions/segments before plotting
- Same pipeline for solar-system and deep-sky (and later ISS)

## Stability chores

Not user-facing features, but required so the priority list does not rot the tree:

- Extract domain/infra behind `lib.rs` when DSO/ISS share types
- [x] Fix window title (`"egui Demo"` → AstroCalc) and app icon
- Migration/schema hygiene (empty third migration; naming leftovers)
- [x] Cull unused deps (`clap`, `suncalc`, `polars`)
- [x] CI: `fmt` + `clippy -D warnings` + `test` on PRs (`.github/workflows/ci.yml`)
- Keep [modules.md](modules.md) and this roadmap updated when a major feature lands
- [x] Config panel UX: compact layout, sticky Save, OSM location map with offline no-tiles fallback

## Out of scope for now

- Mobile / web UI
- Multi-user cloud sync
- Replacing egui or Diesel
- ISS conjunction mode (ISS near planet / deep-sky target)
- Telescope hardware profile and expected frame / photo overlay preview
- Additional deep-sky planning work beyond what is already shipped
