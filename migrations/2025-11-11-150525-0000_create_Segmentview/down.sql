-- This file should undo anything in `up.sql`

CREATE TABLE `posts`(
	`id` INTEGER NOT NULL PRIMARY KEY,
	`title` TEXT NOT NULL,
	`body` TEXT NOT NULL,
	`published` BOOL NOT NULL
);

CREATE TABLE `viewsegement`(
	`name` TEXT NOT NULL PRIMARY KEY,
	`duration` DOUBLE NOT NULL,
	`date` TEXT NOT NULL,
	`lon` DOUBLE NOT NULL,
	`lat` DOUBLE NOT NULL
);

DROP TABLE IF EXISTS `dateinfo`;
