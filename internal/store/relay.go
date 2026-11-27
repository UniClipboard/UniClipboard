package store

import (
	"database/sql"
	"errors"
	"time"
)

const relaySchema = `
CREATE TABLE IF NOT EXISTS device_relay_routes (
  device_id      TEXT PRIMARY KEY REFERENCES devices (id) ON DELETE CASCADE,
  relay_origin   TEXT NOT NULL,
  mailbox_id     TEXT NOT NULL,
  delivery_token TEXT NOT NULL,
  max_bytes      INTEGER NOT NULL DEFAULT 0,
  epoch          INTEGER NOT NULL DEFAULT 1,
  updated_ms     INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS device_relay_tokens (
  device_id      TEXT PRIMARY KEY REFERENCES devices (id) ON DELETE CASCADE,
  relay_origin   TEXT NOT NULL,
  delivery_token TEXT NOT NULL,
  token_hash     TEXT NOT NULL,
  epoch          INTEGER NOT NULL DEFAULT 1,
  created_ms     INTEGER NOT NULL
) WITHOUT ROWID;
`

var (
	ErrRelayRouteNotFound = errors.New("store: no relay route for device")
	ErrRelayTokenNotFound = errors.New("store: no relay token for device")
)

// RelayRoute is the secret routing information a peer advertised so this device
// can upload E2EE transfers to its mailbox while P2P is offline.
type RelayRoute struct {
	DeviceID      string
	RelayOrigin   string
	MailboxID     string
	DeliveryToken string
	MaxBytes      int64
	Epoch         uint64
	UpdatedMs     int64
}

// RelayToken is the per-peer delivery token this device issued for its own mailbox.
type RelayToken struct {
	DeviceID      string
	RelayOrigin   string
	DeliveryToken string
	TokenHash     string
	Epoch         uint64
	CreatedMs     int64
}

func (s *Store) UpsertRelayRoute(r RelayRoute) error {
	if r.UpdatedMs <= 0 {
		r.UpdatedMs = time.Now().UnixMilli()
	}
	_, err := s.db.Exec(`INSERT INTO device_relay_routes (device_id, relay_origin, mailbox_id, delivery_token, max_bytes, epoch, updated_ms)
		VALUES (?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(device_id) DO UPDATE SET
			relay_origin = excluded.relay_origin,
			mailbox_id = excluded.mailbox_id,
			delivery_token = excluded.delivery_token,
			max_bytes = excluded.max_bytes,
			epoch = excluded.epoch,
			updated_ms = excluded.updated_ms`,
		r.DeviceID, r.RelayOrigin, r.MailboxID, r.DeliveryToken, r.MaxBytes, r.Epoch, r.UpdatedMs)
	return err
}

func (s *Store) RelayRoute(deviceID string) (RelayRoute, error) {
	var r RelayRoute
	err := s.db.QueryRow(`SELECT device_id, relay_origin, mailbox_id, delivery_token, max_bytes, epoch, updated_ms
		FROM device_relay_routes WHERE device_id = ?`, deviceID).
		Scan(&r.DeviceID, &r.RelayOrigin, &r.MailboxID, &r.DeliveryToken, &r.MaxBytes, &r.Epoch, &r.UpdatedMs)
	if errors.Is(err, sql.ErrNoRows) {
		return RelayRoute{}, ErrRelayRouteNotFound
	}
	return r, err
}

func (s *Store) DeleteRelayRoute(deviceID string) error {
	_, err := s.db.Exec(`DELETE FROM device_relay_routes WHERE device_id = ?`, deviceID)
	return err
}

func (s *Store) UpsertRelayToken(t RelayToken) error {
	if t.CreatedMs <= 0 {
		t.CreatedMs = time.Now().UnixMilli()
	}
	_, err := s.db.Exec(`INSERT INTO device_relay_tokens (device_id, relay_origin, delivery_token, token_hash, epoch, created_ms)
		VALUES (?, ?, ?, ?, ?, ?)
		ON CONFLICT(device_id) DO UPDATE SET
			relay_origin = excluded.relay_origin,
			delivery_token = excluded.delivery_token,
			token_hash = excluded.token_hash,
			epoch = excluded.epoch,
			created_ms = excluded.created_ms`,
		t.DeviceID, t.RelayOrigin, t.DeliveryToken, t.TokenHash, t.Epoch, t.CreatedMs)
	return err
}

func (s *Store) RelayToken(deviceID string) (RelayToken, error) {
	var t RelayToken
	err := s.db.QueryRow(`SELECT device_id, relay_origin, delivery_token, token_hash, epoch, created_ms
		FROM device_relay_tokens WHERE device_id = ?`, deviceID).
		Scan(&t.DeviceID, &t.RelayOrigin, &t.DeliveryToken, &t.TokenHash, &t.Epoch, &t.CreatedMs)
	if errors.Is(err, sql.ErrNoRows) {
		return RelayToken{}, ErrRelayTokenNotFound
	}
	return t, err
}

func (s *Store) DeleteRelayToken(deviceID string) (RelayToken, error) {
	tok, err := s.RelayToken(deviceID)
	if err != nil {
		return RelayToken{}, err
	}
	if _, err := s.db.Exec(`DELETE FROM device_relay_tokens WHERE device_id = ?`, deviceID); err != nil {
		return RelayToken{}, err
	}
	return tok, nil
}

func (s *Store) RelayTokens() ([]RelayToken, error) {
	rows, err := s.db.Query(`SELECT device_id, relay_origin, delivery_token, token_hash, epoch, created_ms FROM device_relay_tokens`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []RelayToken
	for rows.Next() {
		var t RelayToken
		if err := rows.Scan(&t.DeviceID, &t.RelayOrigin, &t.DeliveryToken, &t.TokenHash, &t.Epoch, &t.CreatedMs); err != nil {
			return nil, err
		}
		out = append(out, t)
	}
	return out, rows.Err()
}
