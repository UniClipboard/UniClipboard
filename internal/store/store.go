// Package store persists the device identity and the paired peers in SQLite.
package store

import (
	"database/sql"
	"errors"
	"strings"
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
-- Devices that asked to join, or were introduced by a paired device, and wait
-- for the user. state is 'pending' or 'rejected' (remembered so that
-- introductions do not nag again).
CREATE TABLE IF NOT EXISTS pending (
  id    TEXT PRIMARY KEY,
  name  TEXT NOT NULL,
  addr  TEXT NOT NULL,
  via   TEXT NOT NULL DEFAULT '',
  state TEXT NOT NULL DEFAULT 'pending',
  seen  INTEGER NOT NULL
);
`

// migrations bring databases created by older versions up to date. Each one
// may fail only because it already ran.
var migrations = []string{
	// by_code: the device found this one through the pairing code it showed.
	`ALTER TABLE pending ADD COLUMN by_code INTEGER NOT NULL DEFAULT 0`,
}

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
	for _, m := range migrations {
		if _, err := db.Exec(m); err != nil && !strings.Contains(err.Error(), "duplicate column") {
			db.Close()
			return nil, err
		}
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

// Candidate is a device waiting for the user's decision.
type Candidate struct {
	ID, Name, Addr string
	Via            string // name of the paired device that introduced it; empty if it connected directly
	ByCode         bool   // it connected with the pairing code this device was showing
	Rejected       bool
	Seen           time.Time
}

// HasPeer reports whether id is a paired device.
func (s *Store) HasPeer(id string) (bool, error) {
	var n int
	err := s.db.QueryRow(`SELECT COUNT(*) FROM peers WHERE id = ?`, id).Scan(&n)
	return n > 0, err
}

// AddCandidate records c as pending. A candidate the user rejected stays
// rejected when merely introduced again (revive false); a device that connects
// by itself (revive true) asks again, so it comes back as pending.
// It reports whether the candidate is new or was revived, i.e. whether the
// user should be told.
func (s *Store) AddCandidate(c Candidate, revive bool) (bool, error) {
	var state string
	err := s.db.QueryRow(`SELECT state FROM pending WHERE id = ?`, c.ID).Scan(&state)
	switch {
	case errors.Is(err, sql.ErrNoRows):
		_, err = s.db.Exec(`INSERT INTO pending (id, name, addr, via, by_code, state, seen) VALUES (?, ?, ?, ?, ?, 'pending', ?)`,
			c.ID, c.Name, c.Addr, c.Via, c.ByCode, time.Now().Unix())
		return err == nil, err
	case err != nil:
		return false, err
	case state == "rejected" && !revive:
		return false, nil
	}
	_, err = s.db.Exec(`UPDATE pending SET name = ?, addr = ?, via = ?, by_code = ?, state = 'pending', seen = ? WHERE id = ?`,
		c.Name, c.Addr, c.Via, c.ByCode, time.Now().Unix(), c.ID)
	return state == "rejected", err
}

// Candidates returns devices waiting for a decision.
func (s *Store) Candidates() ([]Candidate, error) {
	rows, err := s.db.Query(`SELECT id, name, addr, via, by_code, seen FROM pending WHERE state = 'pending' ORDER BY seen`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []Candidate
	for rows.Next() {
		var c Candidate
		var seen int64
		if err := rows.Scan(&c.ID, &c.Name, &c.Addr, &c.Via, &c.ByCode, &seen); err != nil {
			return nil, err
		}
		c.Seen = time.Unix(seen, 0)
		out = append(out, c)
	}
	return out, rows.Err()
}

// Candidate returns the pending candidate id, or ok=false.
func (s *Store) Candidate(id string) (c Candidate, ok bool, err error) {
	var seen int64
	err = s.db.QueryRow(`SELECT id, name, addr, via, by_code, seen FROM pending WHERE id = ? AND state = 'pending'`, id).
		Scan(&c.ID, &c.Name, &c.Addr, &c.Via, &c.ByCode, &seen)
	if errors.Is(err, sql.ErrNoRows) {
		return c, false, nil
	}
	c.Seen = time.Unix(seen, 0)
	return c, err == nil, err
}

func (s *Store) RejectCandidate(id string) (bool, error) {
	r, err := s.db.Exec(`UPDATE pending SET state = 'rejected' WHERE id = ? AND state = 'pending'`, id)
	if err != nil {
		return false, err
	}
	n, _ := r.RowsAffected()
	return n > 0, nil
}

func (s *Store) DeleteCandidate(id string) error {
	_, err := s.db.Exec(`DELETE FROM pending WHERE id = ?`, id)
	return err
}
