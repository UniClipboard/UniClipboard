-- Rollback: recreate paired_device with the final schema it had before drop,
-- matching 2026-01-24-000000_create_paired_device + 2026-02-03-000001_add_paired_device_name
-- + 2026-03-11-000001_add_paired_device_sync_settings. Table will come back empty;
-- the 2026-04-18-000001_create_space_member backfill cannot be replayed in the
-- reverse direction, so downgrade leaves space_member as the single source of truth.
CREATE TABLE paired_device (
    peer_id TEXT PRIMARY KEY NOT NULL,
    pairing_state TEXT NOT NULL,
    identity_fingerprint TEXT NOT NULL,
    paired_at INTEGER NOT NULL,
    last_seen_at INTEGER,
    device_name TEXT NOT NULL DEFAULT 'Unknown Device',
    sync_settings TEXT DEFAULT NULL
);
