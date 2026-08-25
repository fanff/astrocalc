CREATE TABLE `config_profiles` (
    `id` INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    `name` TEXT NOT NULL COLLATE NOCASE UNIQUE,
    `lat` DOUBLE NOT NULL,
    `lon` DOUBLE NOT NULL,
    `view_windows_json` TEXT NOT NULL,
    `bortle_class` INTEGER NOT NULL DEFAULT 5
);

CREATE TABLE `app_state` (
    `id` INTEGER NOT NULL PRIMARY KEY CHECK (`id` = 1),
    `active_profile_id` INTEGER NOT NULL,
    FOREIGN KEY (`active_profile_id`) REFERENCES `config_profiles` (`id`)
);

INSERT INTO `config_profiles` (`name`, `lat`, `lon`, `view_windows_json`, `bortle_class`)
SELECT 'Default', `lat`, `lon`, `view_windows_json`, `bortle_class`
FROM `app_settings`
WHERE `id` = 1;

INSERT INTO `app_state` (`id`, `active_profile_id`)
SELECT 1, `id`
FROM `config_profiles`
ORDER BY `id`
LIMIT 1;

DROP TABLE `app_settings`;
