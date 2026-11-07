// Package store persists the device identity and the paired peers in SQLite.
package store

import (
	"database/sql"
	"errors"
	"time"

	_ "modernc.org/sqlite"
)

type Store struct{ db *sql.DB }

type Peer struct {
	ID       string // device id
	Name     string
	Addr     string // the peer's tailcat address (a secret)
	AddedAt  time.Time
	LastSeen time.Time
}

const schema = `
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS peers (
  id        TEXT PRIMARY KEY,
  name      TEXT NOT NULL,
  addr      TEXT NOT NULL,
  added_at  INTEGER NOT NULL,
  last_seen INTEGER NOT NULL DEFAULT 0
);
`

func Open(path string) (*Store, error) {
	// A single connection serializes access; the load is tiny.
	db, err := sql.Open("sqlite", "file:"+path+"?_pragma=busy_timeout(5000)&_pragma=journal_mode(WAL)")
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	if _, err := db.Exec(schema); err != nil {
		db.Close()
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

// UpsertPeer adds a peer or refreshes its name and address.
func (s *Store) UpsertPeer(p Peer) error {
	_, err := s.db.Exec(`
INSERT INTO peers (id, name, addr, added_at) VALUES (?, ?, ?, ?)
ON CONFLICT(id) DO UPDATE SET name = excluded.name, addr = excluded.addr`,
		p.ID, p.Name, p.Addr, time.Now().Unix())
	return err
}

func (s *Store) TouchPeer(id string) error {
	_, err := s.db.Exec(`UPDATE peers SET last_seen = ? WHERE id = ?`, time.Now().Unix(), id)
	return err
}

func (s *Store) DeletePeer(id string) (bool, error) {
	r, err := s.db.Exec(`DELETE FROM peers WHERE id = ?`, id)
	if err != nil {
		return false, err
	}
	n, _ := r.RowsAffected()
	return n > 0, nil
}

func (s *Store) Peers() ([]Peer, error) {
	rows, err := s.db.Query(`SELECT id, name, addr, added_at, last_seen FROM peers ORDER BY added_at`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []Peer
	for rows.Next() {
		var p Peer
		var a, l int64
		if err := rows.Scan(&p.ID, &p.Name, &p.Addr, &a, &l); err != nil {
			return nil, err
		}
		p.AddedAt = time.Unix(a, 0)
		if l > 0 {
			p.LastSeen = time.Unix(l, 0)
		}
		out = append(out, p)
	}
	return out, rows.Err()
}
