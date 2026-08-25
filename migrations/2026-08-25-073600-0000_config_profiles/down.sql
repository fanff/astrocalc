CREATE TABLE `app_settings` (
    `id` INTEGER NOT NULL PRIMARY KEY CHECK (`id` = 1),
    `lat` DOUBLE NOT NULL,
    `lon` DOUBLE NOT NULL,
    `view_windows_json` TEXT NOT NULL,
    `bortle_class` INTEGER NOT NULL DEFAULT 5
);

INSERT INTO `app_settings` (`id`, `lat`, `lon`, `view_windows_json`, `bortle_class`)
SELECT 1, p.`lat`, p.`lon`, p.`view_windows_json`, p.`bortle_class`
FROM `config_profiles` p
JOIN `app_state` s ON s.`active_profile_id` = p.`id`
WHERE s.`id` = 1;

DROP TABLE `app_state`;
DROP TABLE `config_profiles`;
