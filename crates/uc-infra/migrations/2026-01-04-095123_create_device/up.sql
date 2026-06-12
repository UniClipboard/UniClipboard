-- Your SQL goes here
CREATE TABLE t_device (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    platform TEXT NOT NULL,
    is_local BOOL NOT NULL,
    created_at BIGINT NOT NULL
);
