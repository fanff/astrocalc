# Roadmap

Prioritized backlog for AstroCalc. Order is intentional: finish data that already exists in the crate, then unify filters, then add satellites and hardware preview.

## Priority features

### 1. Weather in the UI

Integrate the existing Open-Meteo + YAML cache into the desktop UI cleanly.

- [x] Fetch/display forecast for the selected location (and date/night context) — Daily panel
- [x] Surface **cloud coverage**, **humidity**, and **wind** (speed/direction as available from the API)
- [x] Show cache freshness and geo-sector so users know when data was snapped/fetched
- [ ] Use weather as an input to planning (at least visible; later as a visibility/quality filter)

**Depends on:** [src/weather_cache.rs](../src/weather_cache.rs) (`WeatherSnapshot`), Daily widget [src/widgets/weather.rs](../src/widgets/weather.rs), async Bind in `AstroCalcApp`.

### 2. Finish deep-sky

Complete the DSO path from catalog to sky views.

- [x] Parse RA/Dec strings and alt-az conversion for the observer
- [ ] Apply magnitude filtering from catalog `v_mag` as a planning constraint (catalog UI mag limits exist)
- [x] Wire into Daily and Night Tracks with object selection
- [x] SQLite/cache strategy: `objectposition.kind = 'dso'` blobs with selected-id merge via `ensure_dso_positions`

**Depends on:** [src/deepsky/](../src/deepsky/).

### 3. Finish Sun / Moon / planets

Close gaps in the solar-system family.

- Complete Sun handling alongside planets and Moon
- Verify magnitudes, phase (Moon), and night/day edge cases
- [x] ~~**Long Term** panel~~ removed — presence overview superseded by Night Tracks
- [x] **Night Tracks** panel: multi-night timeline (night hours × dates; twilight clear-sky background; altitude-encoded object segments; no weather)
- [x] Solar System calc panel removed — Daily prefetches selected night + 10 days in background
- Keep Daily and Night Tracks on the same position types and filters

**Depends on:** [src/solarsystemcalc.rs](../src/solarsystemcalc.rs), [src/panels/night_tracks.rs](../src/panels/night_tracks.rs).

### 4. FOV, limit magnitude, visibility filtering

Settings-driven constraints applied uniformly across object kinds.

- Persist FOV and limit magnitude in `app_settings` (not panel-only state)
- Filter positions/segments before plotting
- Same pipeline for solar-system and deep-sky (and later ISS)

### 5. ISS view and conjunction

- [x] **Visible (sunlit) passes:** dusk/dawn ISS when sunlit and sky dark; view-window filter; TLE from Celestrak + on-disk cache; dedicated **ISS** panel (~60-day list)
- [x] **Sun / Moon disk transit / near-miss:** sub-second refine; TLE freshness warnings (corridor ~5–10 km)
- [x] **Brightness / quality:** phase angle + approximate magnitude (range/phase/airmass); duration = sunlit+dark visible window; Bortle (Config) + cloud labels from weather forecast (advisory, not hard filters)
- Accuracy: SGP4 + fresh TLE is enough for naked-eye passes; disk transits need TLE ≪12 h and full-precision lat/lon (not 0.01° sector)
- Cache: `iss_events` table + `my_tle_cache/`; invalidate on TLE refresh / site change
- [ ] **Conjunction mode:** ISS near a chosen planet / deep-sky target (angular criteria) — follow-on

**Depends on:** [src/satellites/](../src/satellites/), ISS panel [src/panels/iss.rs](../src/panels/iss.rs).

### 6. Telescope hardware and expected picture preview

- Hardware profile: aperture, focal length, camera/sensor (pixel size, resolution)
- Expected frame / plate-scale preview from FOV + target
- Location **photo overlay** for efficient searching (assets exist under `background/raw_paris/` but are unused)
- Preview shows what is realistically capturable given limit magnitude and framing

## Stability chores

Not user-facing features, but required so the priority list does not rot the tree:

- Extract domain/infra behind `lib.rs` when DSO/ISS share types
- [x] Fix window title (`"egui Demo"` → AstroCalc) and app icon
- Migration/schema hygiene (empty third migration; naming leftovers)
- [x] Cull unused deps (`clap`, `suncalc`, `polars`)
- [x] CI: `fmt` + `clippy -D warnings` + `test` on PRs (`.github/workflows/ci.yml`)
- Keep [modules.md](modules.md) and this roadmap updated when a major feature lands
- Photo asset pipeline (naming, association to site/view window) before overlay UI
- [x] Config panel UX: compact layout, sticky Save, OSM + offline vector basemap (Natural Earth / FR regions)

## Out of scope for now

- Mobile / web UI
- Multi-user cloud sync
- Replacing egui or Diesel
