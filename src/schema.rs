// @generated automatically by Diesel CLI.

diesel::table! {
    app_state (id) {
        id -> Integer,
        active_profile_id -> Integer,
    }
}

diesel::table! {
    config_profiles (id) {
        id -> Integer,
        name -> Text,
        lat -> Double,
        lon -> Double,
        view_windows_json -> Text,
        bortle_class -> Integer,
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

diesel::table! {
    iss_events (id) {
        id -> Integer,
        kind -> Text,
        lat -> Double,
        lon -> Double,
        tle_epoch_ms -> BigInt,
        computed_at_ms -> BigInt,
        start_ms -> BigInt,
        end_ms -> BigInt,
        peak_ms -> BigInt,
        payload_json -> Text,
    }
}

diesel::joinable!(app_state -> config_profiles (active_profile_id));

diesel::allow_tables_to_appear_in_same_query!(
    app_state,
    config_profiles,
    dateinfo,
    objectposition,
    iss_events,
);
