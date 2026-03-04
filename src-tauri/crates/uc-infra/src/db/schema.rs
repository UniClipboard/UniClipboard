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
    paired_device,
    t_device,
);
