CREATE TABLE `iss_events` (
	`id` INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	`kind` TEXT NOT NULL,
	`lat` DOUBLE NOT NULL,
	`lon` DOUBLE NOT NULL,
	`tle_epoch_ms` BIGINT NOT NULL,
	`computed_at_ms` BIGINT NOT NULL,
	`start_ms` BIGINT NOT NULL,
	`end_ms` BIGINT NOT NULL,
	`peak_ms` BIGINT NOT NULL,
	`payload_json` TEXT NOT NULL
);

CREATE INDEX `iss_events_site_kind_peak` ON `iss_events` (`lat`, `lon`, `kind`, `peak_ms`);
