-- Your SQL goes here
CREATE TABLE `viewsegement`(
	`name` TEXT NOT NULL PRIMARY KEY,
	`duration` DOUBLE NOT NULL,
	`date` TEXT NOT NULL,
	`lon` DOUBLE NOT NULL,
	`lat` DOUBLE NOT NULL
);

CREATE TABLE `objectposition`(
	`id` INTEGER NOT NULL PRIMARY KEY,
	`lat_sector` DOUBLE NOT NULL,
	`lon_sector` DOUBLE NOT NULL,
	`data_chunk` BINARY NOT NULL,
	`date` TEXT NOT NULL,
	`calculated_at_ms` BIGINT NOT NULL
);

CREATE TABLE `posts`(
	`id` INTEGER NOT NULL PRIMARY KEY,
	`title` TEXT NOT NULL,
	`body` TEXT NOT NULL,
	`published` BOOL NOT NULL
);

