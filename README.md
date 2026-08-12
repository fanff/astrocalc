# AstroCalc

A desktop observation planner for night-sky photography and visual astronomy.

Pick your site, define the parts of the sky you can actually see, then find when targets are worth looking at — without recomputing the night every time you open the app.

![Daily view — weather, sky map, and visibility timeline](doc/screenshots/dailyview.png)

## What you can do

- **Set up your site** — place yourself on a map and draw visibility zones for your usable sky (trees, roofs, horizon limits)
- **Explore a single night** — sky map and timeline for one evening; selecting a date caches that night plus ~10 days ahead in the background
- **Scan the long term** — multi-night presence overview from cache (objects visible ≥20 minutes in your view), with the same catalog selection as Daily
- **Check observing conditions** — weather context (clouds, humidity, wind) for the selected location and night
- **Work offline for setup** — Config map falls back to an embedded vector basemap (countries, French regions, key cities) when the network is unavailable

## Coming next

- Deeper magnitude / FOV planning filters shared across object kinds
- ISS passes and conjunctions with planets or deep-sky targets
- Telescope field of view, magnitude limits, and expected frame preview

## Status

Early and actively developed. Solar-system and deep-sky nightly planning, Daily visualization, Long Term presence overview, and weather in Daily are usable today. Satellites and hardware preview remain on the roadmap.

## Download

Prebuilt binaries for Windows, macOS (Apple Silicon), and Linux are published on the [Latest release](https://github.com/fanff/astrocalc/releases/latest) page whenever `main` is updated. Grab the archive for your OS, extract it, and run the binary.

## Run

Requires a Rust toolchain.

```bash
cargo run
```

On first run, settings are seeded for Paris center with a near-full north-facing view window. Config **Save** stores location and visibility zones in SQLite (`app_settings`).

Copy `.env.example` to `.env` if you want a local database URL override (`DATABASE_URL`). Migrations under `migrations/` run automatically at startup.

## More detail

Design notes and backlog live under [`doc/`](doc/).
