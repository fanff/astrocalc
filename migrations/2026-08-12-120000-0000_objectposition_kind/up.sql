-- Separate solar-system vs deep-sky position blobs per night × sector.
ALTER TABLE objectposition ADD COLUMN kind TEXT NOT NULL DEFAULT 'solar';

CREATE UNIQUE INDEX objectposition_date_sector_kind
ON objectposition (date, lat_sector, lon_sector, kind);
