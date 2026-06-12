-- Create trusted_peer as the persistent fact-table for "this peer is trusted
-- to communicate with us". The uc-core::trusted_peer domain replaces the old
-- PairedDevice god-object; see TRUSTED_PEER_DOMAIN_ZH.md §4 / §8.
--
-- No data migration from paired_device: the old rows are abandoned on
-- purpose, users re-pair after upgrade.
--
-- Columns map 1:1 to the TrustedPeer aggregate:
--   peer_device_id    := TrustedPeer.peer_device_id  (primary key)
--   local_device_id   := TrustedPeer.local_device_id
--   peer_fingerprint  := TrustedPeer.peer_fingerprint (opaque String)
--   trusted_at        := TrustedPeer.trusted_at      (seconds since epoch,
--                                                     matches project-wide
--                                                     timestamp convention)
--
-- Hard-delete model: distrust removes the row, there is no tombstone column.

CREATE TABLE trusted_peer (
    peer_device_id   TEXT PRIMARY KEY NOT NULL,
    local_device_id  TEXT NOT NULL,
    peer_fingerprint TEXT NOT NULL,
    trusted_at       INTEGER NOT NULL
);

CREATE INDEX idx_trusted_peer_local ON trusted_peer(local_device_id);
