-- Single-row cross-device "active clipboard" LWW register.
--
-- Records which clipboard content is currently the active OS-clipboard
-- content as a last-writer-wins register: keyed across devices by
-- content_hash and ordered by (activated_at_ms, activated_by). The id
-- column is pinned to 1 so the table holds at most one row; the row is
-- absent until the first activation is recorded.
CREATE TABLE active_clipboard_register (
    id               INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    content_hash     TEXT    NOT NULL,
    entry_id         TEXT    NOT NULL,
    activated_at_ms  BIGINT  NOT NULL,
    activated_by     TEXT    NOT NULL
);
