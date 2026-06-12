-- This file should undo anything in `up.sql`
PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS clipboard_selection;
DROP TABLE IF EXISTS clipboard_entry;
DROP TABLE IF EXISTS clipboard_snapshot_representation;
DROP TABLE IF EXISTS blob;
DROP TABLE IF EXISTS clipboard_event;
