# Vector basemap (offline)

Embedded GeoJSON used by Config when OSM tiles are unavailable.

| File | Source | Role |
|------|--------|------|
| `ne_110m_admin_0_countries.geojson` | [Natural Earth](https://www.naturalearthdata.com/) 1:110m | World country outlines |
| `france_admin1.geojson` | [france-geojson](https://github.com/gregoiredavid/france-geojson) simplified regions | French régions |
| `places.geojson` | Natural Earth populated places (filtered) | Capitals + major cities |

Natural Earth data is public domain. France regions GeoJSON is typically MIT/open (see upstream).

No Python or raster tile dumps — vectors are drawn in Rust via `src/widgets/vector_basemap.rs`.
