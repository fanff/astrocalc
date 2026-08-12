-- Your SQL goes here

DROP TABLE IF EXISTS `posts`;
DROP TABLE IF EXISTS `viewsegement`;
CREATE TABLE `dateinfo`(
	`id` INTEGER NOT NULL PRIMARY KEY,
	`date` TEXT NOT NULL,
	`lat_sector` DOUBLE NOT NULL,
	`lon_sector` DOUBLE NOT NULL,
	`night_start_ms` BIGINT NOT NULL,
	`night_end_ms` BIGINT NOT NULL
);

