DROP INDEX IF EXISTS objectposition_date_sector_kind;

-- SQLite cannot DROP COLUMN on older targets reliably; rebuild without kind.
CREATE TABLE objectposition_old AS SELECT
    id, lat_sector, lon_sector, data_chunk, date, calculated_at_ms
FROM objectposition;

DROP TABLE objectposition;

CREATE TABLE objectposition (
    id INTEGER NOT NULL PRIMARY KEY,
    lat_sector DOUBLE NOT NULL,
    lon_sector DOUBLE NOT NULL,
    data_chunk BINARY NOT NULL,
    date TEXT NOT NULL,
    calculated_at_ms BIGINT NOT NULL
);

INSERT INTO objectposition (id, lat_sector, lon_sector, data_chunk, date, calculated_at_ms)
SELECT id, lat_sector, lon_sector, data_chunk, date, calculated_at_ms FROM objectposition_old;

DROP TABLE objectposition_old;
