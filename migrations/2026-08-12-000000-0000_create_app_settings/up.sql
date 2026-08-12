CREATE TABLE `app_settings` (
	`id` INTEGER NOT NULL PRIMARY KEY CHECK (`id` = 1),
	`lat` DOUBLE NOT NULL,
	`lon` DOUBLE NOT NULL,
	`view_windows_json` TEXT NOT NULL
);
