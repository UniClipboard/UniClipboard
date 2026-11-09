// Package store persists the device identity and the paired peers in SQLite.
package store

import (
	"database/sql"
	"errors"

	_ "modernc.org/sqlite"
)

type Store struct{ db *sql.DB }

const schema = `
CREATE TABLE IF NOT EXISTS file_cache (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, size INTEGER NOT NULL,
 completed_ms INTEGER NOT NULL, available INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS history_imports (
 source TEXT NOT NULL, record_id TEXT NOT NULL, history_id TEXT NOT NULL,
 PRIMARY KEY(source, record_id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS history_labels (name TEXT PRIMARY KEY) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS clip_item_order (
 item_seq INTEGER PRIMARY KEY REFERENCES clip_items(seq) ON DELETE CASCADE,
 order_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS clip_item_order_time ON clip_item_order(order_ms, item_seq);
CREATE TABLE IF NOT EXISTS history_import_limits (
 id INTEGER PRIMARY KEY CHECK(id=1), max_items INTEGER NOT NULL,
 max_bytes INTEGER NOT NULL, max_stored_bytes INTEGER NOT NULL
);
`

func Open(path string) (*Store, error) {
	// A single connection serializes access; the load is tiny.
	db, err := sql.Open("sqlite", "file:"+path+"?_pragma=busy_timeout(5000)&_pragma=journal_mode(WAL)&_pragma=foreign_keys(1)")
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	if _, err := db.Exec(schema); err != nil {
		_ = db.Close()
		return nil, err
	}
	if err := migrate(db); err != nil {
		_ = db.Close()
		return nil, err
	}
	return &Store{db}, nil
}

func (s *Store) Close() error { return s.db.Close() }

// Meta returns the stored value, or nil if the key is absent.
func (s *Store) Meta(k string) ([]byte, error) {
	var v []byte
	err := s.db.QueryRow(`SELECT v FROM meta WHERE k = ?`, k).Scan(&v)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	return v, err
}

func (s *Store) SetMeta(k string, v []byte) error {
	_, err := s.db.Exec(`INSERT INTO meta (k, v) VALUES (?, ?) ON CONFLICT(k) DO UPDATE SET v = excluded.v`, k, v)
	return err
}
