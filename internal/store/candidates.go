package store

import (
	"database/sql"
	"errors"
)

// Candidates are devices an approved peer vouched for (decision 0012). They are
// suggestions: nothing here is trust, and the address is a secret.

const candidateSchema = `CREATE TABLE candidates (
  device_id     TEXT NOT NULL,
  instance_id   TEXT NOT NULL,
  introducer_id TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
  name          TEXT NOT NULL DEFAULT '',   -- what the introducer calls it; not verified
  address       BLOB NOT NULL,              -- a secret
  seen_ms       INTEGER NOT NULL,           -- when the introducer last vouched for it
  PRIMARY KEY (device_id, instance_id, introducer_id)
);
CREATE INDEX candidates_seen ON candidates (seen_ms);
CREATE TABLE candidate_ignored (
  device_id TEXT PRIMARY KEY,
  ms        INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE candidate_requests (
  device_id    TEXT PRIMARY KEY,
  instance_id  TEXT NOT NULL,
  requested_ms INTEGER NOT NULL
) WITHOUT ROWID;`

// Caps on stored candidates.
const (
	MaxCandidates              = 256
	MaxCandidatesPerIntroducer = 128
)

// Candidate is one vouch: introducer says device is reachable at address.
type Candidate struct {
	DeviceID, InstanceID, IntroducerID string
	Name                               string
	Address                            []byte
	SeenMs                             int64
}

// ReplaceVouches records what introducer vouches for on one instance now.
// Devices that already have a record are skipped. Earlier vouches that are not
// repeated keep their old seen time, so they age out on their own.
func (s *Store) ReplaceVouches(introducer, instance string, cs []Candidate, now, ttlMs int64) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	for _, c := range cs {
		var one int
		if err := tx.QueryRow(`SELECT 1 FROM devices WHERE id = ?`, c.DeviceID).Scan(&one); err == nil {
			continue
		} else if !errors.Is(err, sql.ErrNoRows) {
			return err
		}
		if _, err := tx.Exec(`INSERT INTO candidates (device_id, instance_id, introducer_id, name, address, seen_ms) VALUES (?,?,?,?,?,?)
ON CONFLICT(device_id, instance_id, introducer_id) DO UPDATE SET name = excluded.name, address = excluded.address, seen_ms = excluded.seen_ms`,
			c.DeviceID, instance, introducer, c.Name, c.Address, now); err != nil {
			return err
		}
	}
	if _, err := tx.Exec(`DELETE FROM candidates WHERE seen_ms < ?`, now-ttlMs); err != nil {
		return err
	}
	if _, err := tx.Exec(`DELETE FROM candidates WHERE introducer_id = ? AND rowid NOT IN
(SELECT rowid FROM candidates WHERE introducer_id = ? ORDER BY seen_ms DESC LIMIT ?)`, introducer, introducer, MaxCandidatesPerIntroducer); err != nil {
		return err
	}
	if _, err := tx.Exec(`DELETE FROM candidates WHERE rowid NOT IN (SELECT rowid FROM candidates ORDER BY seen_ms DESC LIMIT ?)`, MaxCandidates); err != nil {
		return err
	}
	return tx.Commit()
}

// DropIntroducer forgets everything one device vouched for.
func (s *Store) DropIntroducer(id string) error {
	_, err := s.db.Exec(`DELETE FROM candidates WHERE introducer_id = ?`, id)
	return err
}

// Candidates lists vouches not older than ttl for devices without a record.
func (s *Store) Candidates(now, ttlMs int64) ([]Candidate, error) {
	rows, err := s.db.Query(`SELECT device_id, instance_id, introducer_id, name, address, seen_ms FROM candidates
WHERE seen_ms >= ? AND device_id NOT IN (SELECT id FROM devices) ORDER BY device_id, instance_id, seen_ms DESC`, now-ttlMs)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []Candidate
	for rows.Next() {
		var c Candidate
		if err := rows.Scan(&c.DeviceID, &c.InstanceID, &c.IntroducerID, &c.Name, &c.Address, &c.SeenMs); err != nil {
			return nil, err
		}
		out = append(out, c)
	}
	return out, rows.Err()
}

// SetCandidateIgnored hides or restores a candidate.
func (s *Store) SetCandidateIgnored(deviceID string, ignored bool, now int64) error {
	if ignored {
		_, err := s.db.Exec(`INSERT INTO candidate_ignored (device_id, ms) VALUES (?,?) ON CONFLICT(device_id) DO UPDATE SET ms = excluded.ms`, deviceID, now)
		return err
	}
	_, err := s.db.Exec(`DELETE FROM candidate_ignored WHERE device_id = ?`, deviceID)
	return err
}

// IgnoredCandidates is the set of ignored device IDs.
func (s *Store) IgnoredCandidates() (map[string]bool, error) {
	rows, err := s.db.Query(`SELECT device_id FROM candidate_ignored`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	out := map[string]bool{}
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return nil, err
		}
		out[id] = true
	}
	return out, rows.Err()
}

// RequestCandidate records that the user wants to pair with a candidate.
func (s *Store) RequestCandidate(deviceID, instance string, now int64) error {
	_, err := s.db.Exec(`INSERT INTO candidate_requests (device_id, instance_id, requested_ms) VALUES (?,?,?)
ON CONFLICT(device_id) DO UPDATE SET instance_id = excluded.instance_id, requested_ms = excluded.requested_ms`, deviceID, instance, now)
	return err
}

// CandidateRequest is a wish to pair that has not succeeded yet.
type CandidateRequest struct {
	DeviceID, InstanceID string
	RequestedMs          int64
}

// CandidateRequests lists requests not older than windowMs.
func (s *Store) CandidateRequests(now, windowMs int64) ([]CandidateRequest, error) {
	if _, err := s.db.Exec(`DELETE FROM candidate_requests WHERE requested_ms < ?`, now-windowMs); err != nil {
		return nil, err
	}
	rows, err := s.db.Query(`SELECT device_id, instance_id, requested_ms FROM candidate_requests`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []CandidateRequest
	for rows.Next() {
		var r CandidateRequest
		if err := rows.Scan(&r.DeviceID, &r.InstanceID, &r.RequestedMs); err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// ClearCandidateRequest drops a request (it succeeded or was withdrawn).
func (s *Store) ClearCandidateRequest(deviceID string) error {
	_, err := s.db.Exec(`DELETE FROM candidate_requests WHERE device_id = ?`, deviceID)
	return err
}
