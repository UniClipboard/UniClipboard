-- Create peer_address: last-observed iroh transport address per paired device.
--
-- Slice 2 Phase 1. Consumed by F1 `ensure_reachable_all` so the roster can
-- dial every member right after `start_network` without depending on
-- rendezvous / mDNS resolution.
--
-- Columns map 1:1 to `uc-core::ports::peer_address::PeerAddressRecord`:
--   device_id    := PeerAddressRecord.device_id       (primary key; matches
--                                                      space_member.device_id
--                                                      but no FK — we keep the
--                                                      address around even if
--                                                      the member row is
--                                                      briefly missing during
--                                                      setup races)
--   addr_blob    := PeerAddressRecord.addr_blob       (adapter-encoded
--                                                      iroh::NodeAddr bytes —
--                                                      core treats as opaque)
--   observed_at  := PeerAddressRecord.observed_at     (unix seconds, matches
--                                                      project-wide timestamp
--                                                      convention; see
--                                                      trusted_peer.trusted_at)
--
-- Upsert semantics: last-write-wins on device_id (the port contract).

CREATE TABLE peer_address (
    device_id   TEXT PRIMARY KEY NOT NULL,
    addr_blob   BLOB NOT NULL,
    observed_at INTEGER NOT NULL
);
