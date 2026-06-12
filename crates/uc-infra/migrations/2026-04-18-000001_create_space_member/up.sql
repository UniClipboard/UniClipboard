-- Create space_member as the replacement for paired_device.
--
-- Phase 1 of the paired_device → membership migration: the new table lives
-- alongside paired_device. A one-shot data migration below copies every
-- Trusted row from paired_device so the new table starts populated.
--
-- Mapping decisions (see design discussion 2026-04-18):
--   device_id             := peer_id                       (direct string reuse)
--   device_name           := device_name                   (unchanged)
--   identity_fingerprint  := identity_fingerprint          (unchanged)
--   joined_at             := paired_at                     (seconds since epoch)
--   sync_preferences      := derived from sync_settings    (see CASE below)
--   pairing_state         := dropped                       (only Trusted rows are copied)
--   last_seen_at          := dropped                       (moves to network layer)
--
-- sync_settings → MemberSyncPreferences JSON mapping:
--   NULL sync_settings             → full defaults (all booleans true)
--   sync_settings.auto_sync        → mirrored onto both send_enabled and receive_enabled
--   sync_settings.content_types.*  → mirrored onto both send_content_types and receive_content_types
--   sync_settings.sync_frequency   → dropped (no equivalent in MemberSyncPreferences)
--
-- SQLite stores JSON booleans as integers (0/1) so each boolean field goes
-- through a CASE expression that re-encodes it as a JSON boolean.

CREATE TABLE space_member (
    device_id TEXT PRIMARY KEY NOT NULL,
    device_name TEXT NOT NULL,
    identity_fingerprint TEXT NOT NULL,
    joined_at INTEGER NOT NULL,
    sync_preferences TEXT NOT NULL
);

INSERT INTO space_member (device_id, device_name, identity_fingerprint, joined_at, sync_preferences)
SELECT
    peer_id AS device_id,
    device_name,
    identity_fingerprint,
    paired_at AS joined_at,
    CASE
        WHEN sync_settings IS NULL THEN
            json_object(
                'send_enabled', json('true'),
                'receive_enabled', json('true'),
                'send_content_types', json_object(
                    'text', json('true'),
                    'image', json('true'),
                    'link', json('true'),
                    'file', json('true'),
                    'code_snippet', json('true'),
                    'rich_text', json('true')
                ),
                'receive_content_types', json_object(
                    'text', json('true'),
                    'image', json('true'),
                    'link', json('true'),
                    'file', json('true'),
                    'code_snippet', json('true'),
                    'rich_text', json('true')
                )
            )
        ELSE
            json_object(
                'send_enabled',
                    CASE json_extract(sync_settings, '$.auto_sync')
                        WHEN 0 THEN json('false') ELSE json('true')
                    END,
                'receive_enabled',
                    CASE json_extract(sync_settings, '$.auto_sync')
                        WHEN 0 THEN json('false') ELSE json('true')
                    END,
                'send_content_types', json_object(
                    'text',
                        CASE json_extract(sync_settings, '$.content_types.text')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'image',
                        CASE json_extract(sync_settings, '$.content_types.image')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'link',
                        CASE json_extract(sync_settings, '$.content_types.link')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'file',
                        CASE json_extract(sync_settings, '$.content_types.file')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'code_snippet',
                        CASE json_extract(sync_settings, '$.content_types.code_snippet')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'rich_text',
                        CASE json_extract(sync_settings, '$.content_types.rich_text')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END
                ),
                'receive_content_types', json_object(
                    'text',
                        CASE json_extract(sync_settings, '$.content_types.text')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'image',
                        CASE json_extract(sync_settings, '$.content_types.image')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'link',
                        CASE json_extract(sync_settings, '$.content_types.link')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'file',
                        CASE json_extract(sync_settings, '$.content_types.file')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'code_snippet',
                        CASE json_extract(sync_settings, '$.content_types.code_snippet')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END,
                    'rich_text',
                        CASE json_extract(sync_settings, '$.content_types.rich_text')
                            WHEN 0 THEN json('false') ELSE json('true')
                        END
                )
            )
    END AS sync_preferences
FROM paired_device
WHERE pairing_state = 'Trusted';
