# AstroCalc

A desktop observation planner for night-sky photography and visual astronomy.

Pick your site, define the parts of the sky you can actually see, then find when targets are worth looking at — without recomputing the night every time you open the app.

## What you can do

- **Set up your site** — place yourself on a map and draw visibility zones for your usable sky (trees, roofs, horizon limits)
- **Plan solar-system nights** — precompute when planets and the Moon are above your view for a date range
- **Explore a single night** — sky map and timeline for one evening, filtered by your zones and which objects you care about
- **Check observing conditions** — weather context (clouds, humidity, wind) for the selected location and night
- **Work offline for setup** — Config map falls back to an embedded vector basemap (countries, French regions, key cities) when the network is unavailable

## Coming next

- Deep-sky catalogs (Messier, NGC/IC) in the same planning views
- Multi-night / long-term overviews
- ISS passes and conjunctions with planets or deep-sky targets
- Telescope field of view, magnitude limits, and expected frame preview

## Status

Early and actively developed. Solar-system nightly planning and daily visualization are usable today; weather is shown in the daily view. Deep-sky, long-term plots, satellites, and hardware preview are on the roadmap.

## Run

Requires a Rust toolchain.

```bash
cargo run
```

On first run, settings are seeded for Paris center with a near-full north-facing view window. Config **Save** stores location and visibility zones in SQLite (`app_settings`).

Copy `.env.example` to `.env` if you want a local database URL override (`DATABASE_URL`). Migrations under `migrations/` run automatically at startup.

## More detail

Design notes and backlog live under [`doc/`](doc/).
