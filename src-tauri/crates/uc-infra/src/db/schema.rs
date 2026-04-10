// @generated automatically by Diesel CLI.

diesel::table! {
    blob (blob_id) {
        blob_id -> Text,
        storage_path -> Text,
        storage_backend -> Text,
        size_bytes -> BigInt,
        content_hash -> Text,
        encryption_algo -> Nullable<Text>,
        created_at_ms -> BigInt,
        compressed_size -> Nullable<BigInt>,
    }
}

diesel::table! {
    file_transfer (transfer_id) {
        transfer_id -> Text,
        entry_id -> Text,
        filename -> Text,
        file_size -> Nullable<BigInt>,
        content_hash -> Nullable<Text>,
        status -> Text,
        source_device -> Text,
        cached_path -> Nullable<Text>,
        failure_reason -> Nullable<Text>,
        created_at_ms -> BigInt,
        updated_at_ms -> BigInt,
    }
}

diesel::table! {
    clipboard_entry (entry_id) {
        entry_id -> Text,
        event_id -> Text,
        created_at_ms -> BigInt,
        active_time_ms -> BigInt,
        title -> Nullable<Text>,
        total_size -> BigInt,
        pinned -> Bool,
        deleted_at_ms -> Nullable<BigInt>,
    }
}

diesel::table! {
    clipboard_event (event_id) {
        event_id -> Text,
        captured_at_ms -> BigInt,
        source_device -> Text,
        snapshot_hash -> Text,
    }
}

diesel::table! {
    clipboard_selection (entry_id) {
        entry_id -> Text,
        primary_rep_id -> Text,
        secondary_rep_ids -> Text,
        preview_rep_id -> Text,
        paste_rep_id -> Text,
        policy_version -> Text,
    }
}

diesel::table! {
    clipboard_representation_thumbnail (representation_id) {
        representation_id -> Text,
        thumbnail_blob_id -> Text,
        thumbnail_mime_type -> Text,
        original_width -> Integer,
        original_height -> Integer,
        original_size_bytes -> BigInt,
        created_at_ms -> Nullable<BigInt>,
    }
}

diesel::table! {
    clipboard_snapshot_representation (id) {
        id -> Text,
        event_id -> Text,
        format_id -> Text,
        mime_type -> Nullable<Text>,
        size_bytes -> BigInt,
        inline_data -> Nullable<Binary>,
        blob_id -> Nullable<Text>,
        payload_state -> Text,
        last_error -> Nullable<Text>,
    }
}

diesel::table! {
    t_device (id) {
        id -> Text,
        name -> Text,
        platform -> Text,
        is_local -> Bool,
        created_at -> BigInt,
    }
}

diesel::table! {
    paired_device (peer_id) {
        peer_id -> Text,
        pairing_state -> Text,
        identity_fingerprint -> Text,
        paired_at -> BigInt,
        last_seen_at -> Nullable<BigInt>,
        device_name -> Text,
        sync_settings -> Nullable<Text>,
    }
}

diesel::table! {
    local_metric_daily_count (metric_name, bucket_date) {
        metric_name -> Text,
        bucket_date -> Text,
        count -> BigInt,
        updated_at_ms -> BigInt,
    }
}

diesel::table! {
    local_metric_minute_sample (metric_name, bucket_start_ms) {
        metric_name -> Text,
        bucket_start_ms -> BigInt,
        avg_value -> Double,
        min_value -> Double,
        max_value -> Double,
        last_value -> Double,
        sample_count -> Integer,
        updated_at_ms -> BigInt,
    }
}

diesel::joinable!(clipboard_entry -> clipboard_event (event_id));
diesel::joinable!(clipboard_selection -> clipboard_entry (entry_id));
diesel::joinable!(clipboard_snapshot_representation -> blob (blob_id));
diesel::joinable!(clipboard_snapshot_representation -> clipboard_event (event_id));

diesel::allow_tables_to_appear_in_same_query!(
    blob,
    clipboard_entry,
    clipboard_event,
    clipboard_selection,
    clipboard_representation_thumbnail,
    clipboard_snapshot_representation,
    file_transfer,
    local_metric_daily_count,
    local_metric_minute_sample,
    paired_device,
    t_device,
);
