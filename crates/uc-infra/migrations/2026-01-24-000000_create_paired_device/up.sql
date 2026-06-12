CREATE TABLE paired_device (
    peer_id TEXT PRIMARY KEY NOT NULL,
    pairing_state TEXT NOT NULL,
    identity_fingerprint TEXT NOT NULL,
    paired_at INTEGER NOT NULL,
    last_seen_at INTEGER
);
