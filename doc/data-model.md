# Data model

Sources of truth for configuration, cached ephemeris, weather, and catalogs.

## Named configuration profiles (`AppSettings`)

Each named profile is persisted in SQLite table `config_profiles`; singleton table `app_state` points to the active profile. The settings payload remains `AppSettings` in [src/config.rs](../src/config.rs), with profile CRUD and active selection in [src/models.rs](../src/models.rs).

| Field | Type | Meaning |
|-------|------|---------|
| `lat` | `f64` | Observer latitude (degrees) |
| `lon` | `f64` | Observer longitude (degrees) |
| `view_windows` | `Vec<ViewWindow>` | Az/alt rectangles of usable sky (stored as JSON text) |
| `bortle_class` | `u8` | Bortle dark-sky class `1`…`9` (default `5` suburban); used for ISS naked-eye quality labels |

Connection string is **not** stored here — use `DATABASE_URL` (see Environment).

### First-run defaults

When `config_profiles` has no rows, the app seeds a profile named `Default`:

| Setting | Value |
|---------|--------|
| Location | Paris center `48.8566°N`, `2.3522°E` |
| View window | Wrap-north ≈350°: az `[185 → 175]`, alt `[5, 80]` |
| Bortle | `5` (suburban) |

### ViewWindow

| Field | Meaning |
|-------|---------|
| `min_az_deg` / `max_az_deg` | Azimuth bounds in degrees, typically `[0, 360]`. If `min_az > max_az`, the sector **wraps across north**. |
| `min_alt_deg` / `max_alt_deg` | Altitude bounds (`0 ≤ min < max ≤ 90`) |

`contains(az, alt)` is the geometric membership test used when filtering tracks (wrap-aware).

Edited visually in Config via the polar sky circle ([`src/widgets/view_window_editor.rs`](../src/widgets/view_window_editor.rs)): same N-up projection as Daily (`r = 90 − alt`). Drag yellow corner handles to reshape a zone; **Add zone** creates a default sector. The location map also draws each zone's azimuth wedge and boundary angles around the observer marker; altitude is intentionally omitted from this geographic overlay.

Observer lat/lon is set from the Config location map ([`src/widgets/location_map.rs`](../src/widgets/location_map.rs)): OpenStreetMap tiles when online; offline mode shows a blank map but still accepts click-to-set coordinates. Layout lives in [`src/panels/config.rs`](../src/panels/config.rs). **Save** updates the active profile; **Save as**, **Rename**, and **Delete** manage named profiles. Switching is immediate unless there are unsaved edits, in which case the user chooses save, discard, or cancel.

### Environment

| Variable | Role |
|----------|------|
| `DATABASE_URL` | Diesel connection string (e.g. `database.db` via `.env`) |

### Planned settings extensions

Not in schema yet; add here when implementing roadmap items 4 and 6:

- Limit magnitude, FOV (deg or arcmin)
- Hardware profile (aperture, focal length, sensor)
- Overlay image path(s) tied to site / view window

## SQLite (Diesel)

Schema in [src/schema.rs](../src/schema.rs); row types in [src/models.rs](../src/models.rs). Migrations under `migrations/` (embedded and applied at startup).

### `config_profiles`

Named observer + view configurations. Names are unique case-insensitively.

| Column | Type | Meaning |
|--------|------|---------|
| `id` | integer PK | Generated profile identifier |
| `name` | text | User-facing profile name |
| `lat` / `lon` | double | Observer coordinates |
| `view_windows_json` | text | JSON array of `ViewWindow` |
| `bortle_class` | integer | Bortle class `1`…`9` (default `5`) |

### `app_state`

Singleton application state.

| Column | Type | Meaning |
|--------|------|---------|
| `id` | integer PK | Always `1` (`CHECK (id = 1)`) |
| `active_profile_id` | integer FK | Currently selected `config_profiles.id` |

### `dateinfo`

One night span per calendar date × geo sector.

| Column | Type | Meaning |
|--------|------|---------|
| `id` | integer PK | Surrogate key |
| `date` | text | `YYYY-MM-DD` |
| `lat_sector` / `lon_sector` | double | Snapped observer coordinates |
| `night_start_ms` / `night_end_ms` | bigint | Night bounds as UTC epoch milliseconds |

Maps to domain `NightInfo` via `DateInfo::as_nightinfo`.

### `objectposition`

Cached sampled positions for a date × sector × kind.

| Column | Type | Meaning |
|--------|------|---------|
| `id` | integer PK | Surrogate key |
| `date` | text | `YYYY-MM-DD` |
| `lat_sector` / `lon_sector` | double | Same sectoring as `dateinfo` |
| `data_chunk` | binary | Bincode-encoded position payload |
| `calculated_at_ms` | bigint | When the chunk was written |
| `kind` | text | `'solar'` (planets/Moon) or `'dso'` (deep-sky); default `'solar'` |

Unique index: `(date, lat_sector, lon_sector, kind)` — one blob per family per night × sector.

Lookup pattern: snap lat/lon → query by date + sector + kind → decode blob → filter in domain/UI.

**DSO merge cache:** `ensure_dso_positions` loads the `kind='dso'` blob, computes only **missing** selected display ids, merges, and rewrites the blob. Selection filters what Daily/Night Tracks plot; unselected tracks may remain in the blob for reuse. Changing sampling frequency or night definition requires invalidating DSO (and solar) rows for affected nights.

Solar-system writes use `kind='solar'` only (Daily background prefetch: selected day + 10 nights).

### Position blob (`ObjectPosition`)

Logical fields encoded with bincode (see [src/solarsystemcalc.rs](../src/solarsystemcalc.rs)):

| Field | Meaning |
|-------|---------|
| `name` | Object id (e.g. planet name or `M31`) |
| `utc_datetime` | Sample time (UTC) |
| `date` | Calendar date associated with the night |
| `ra` / `dec` | Equatorial coordinates |
| `altitude` / `azimuth` | Local horizontal frame (degrees) |
| `magnitude` | Apparent magnitude |
| `distance` | Distance (AU-scale for planets) |
| `phase_ratio` | Illuminated fraction (Moon-relevant) |

Chunks are typically `Vec<ObjectPosition>` (or segmented wrappers used when plotting continuous visibility). Changing the encode layout is a **breaking cache change** — bump a version or clear DB.

### Geo sector key

Coordinates rounded to a fixed decimal precision (aligned with weather cache, often 2 decimals). All DB reads/writes for a session should use the same snap function.

## Weather cache

Implemented in [src/weather_cache.rs](../src/weather_cache.rs).

- Directory: `my_weather_app/` (created at startup)
- Freshness: configurable (currently 30 minutes in `main`)
- Files: YAML named roughly `{lat}_{lon}_{unix_ts}.yaml` for snapped location
- Payload: `CachedForecast { location, fetched_at, data }` where `data` wraps API YAML (`ForecastData.yaml_content`)
- Hourly variables requested: `temperature_2m`, `cloud_cover`, `visibility`, `relative_humidity_2m`, `wind_speed_10m`, `wind_direction_10m` (UTC)
- UI-facing type: `WeatherSnapshot { snapped, fetched_at, hourly: Vec<HourlyWeatherPoint> }` returned by `get_weather`
- Daily renders via [src/widgets/weather.rs](../src/widgets/weather.rs); HTTP stays in `AstroCalcApp` + `egui_async::Bind`

## Deep-sky catalog

Embedded CSV: [src/deepsky/ngc-ic-messier-catalog.csv](../src/deepsky/ngc-ic-messier-catalog.csv), loaded once via `once_cell::Lazy` in [src/deepsky/data.rs](../src/deepsky/data.rs).

Notable fields on `DeepObject`: identifiers (NGC/IC/Messier), `ra` / `dec` strings, sizes, `v_mag` (and other bands), constellation, notes. Catalog UI filters by magnitude in [src/widgets/catalog_select.rs](../src/widgets/catalog_select.rs).

DSO position samples are stored in `objectposition` with `kind='dso'` (selected-id merge; see above).

## ISS events and TLE cache

### On-disk TLE

Directory `my_tle_cache/` (gitignored), file `iss_tle.json`: name, line1/line2, `fetched_at`, `tle_epoch`.
Default freshness **6 h**; ISS panel “Refresh orbit data” forces refetch from Celestrak (`CATNR=25544`).
Warn in UI when cache age &gt; 12 h (transits) or &gt; 24 h (passes).
Prediction horizon: **60 calendar days** from the scan start date (UTC midnight).

### `iss_events`

Discrete prediction rows (not night position blobs). Keyed by **full-precision** observer `lat`/`lon` (not 0.01° sector).

| Column | Meaning |
|--------|---------|
| `kind` | `visible_pass` \| `sun_transit` \| `moon_transit` |
| `lat` / `lon` | Observer used for the prediction |
| `tle_epoch_ms` / `computed_at_ms` | Provenance |
| `start_ms` / `end_ms` / `peak_ms` | Event times (UTC ms) |
| `payload_json` | `VisiblePass` or `DiskTransit` serde JSON |

`VisiblePass` payload includes AOS/LOS of the **sunlit + dark-sky visible window**, peak mag/phase/range, and track samples. Older cached rows may omit the new fields (serde defaults).

**Panel open:** cache-first via `IssEventRow::try_load_bundle` / `IssPanelState::reload_cached_only`. On miss (or “Refresh orbit data” / site / view-window change), `egui_async::Bind` runs `fetch_and_predict` and replaces rows for the site.

**Invalidation:** replace all rows for a site when a new prediction bundle is stored (TLE refresh or site/view change triggers recompute in the ISS panel).

## Sample / unused assets

- `background/raw_paris/` — location JPEGs intended for overlay; not referenced by code
- `weather_output.yaml` — sample/output artifact outside the structured cache API

## Extension guidelines

- Prefer new **settings columns / JSON fields** for user settings; new **tables or blob versions** for large computed series.
- ISS uses the `iss_events` event table; do not overload `objectposition` for satellite tracks.
- Always document cache invalidation when sector precision, night definition, or blob layout changes.
