// @generated automatically by Diesel CLI.

diesel::table! {
    app_settings (id) {
        id -> Integer,
        lat -> Double,
        lon -> Double,
        view_windows_json -> Text,
    }
}

diesel::table! {
    dateinfo (id) {
        id -> Integer,
        date -> Text,
        lat_sector -> Double,
        lon_sector -> Double,
        night_start_ms -> BigInt,
        night_end_ms -> BigInt,
    }
}

diesel::table! {
    objectposition (id) {
        id -> Integer,
        lat_sector -> Double,
        lon_sector -> Double,
        data_chunk -> Binary,
        date -> Text,
        calculated_at_ms -> BigInt,
        kind -> Text,
    }
}

diesel::allow_tables_to_appear_in_same_query!(app_settings, dateinfo, objectposition,);
