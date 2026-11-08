package store

import (
	"bytes"
	"database/sql"
	"errors"
	"fmt"
	"time"
)

// Device and binding errors.
var (
	ErrDeviceNotFound   = errors.New("store: no such device")
	ErrDeviceConflict   = errors.New("store: device revision mismatch")
	ErrIdentityConflict = errors.New("store: device id or key already registered with another key")
	ErrBindingNotFound  = errors.New("store: no such binding")
	ErrBindingConflict  = errors.New("store: binding revision mismatch")
)

// Trust values of a device.
const (
	TrustPending = "pending" // known and verified, not yet trusted by the user
	TrustTrusted = "trusted"
	TrustRevoked = "revoked" // tombstone: keeps the key; coming back needs a new approval
)

// Binding states.
const (
	BindingPending    = "pending"
	BindingAuthorized = "authorized"
)

const deviceSchema = `CREATE TABLE devices (
  id            TEXT PRIMARY KEY,
  public_key    BLOB NOT NULL UNIQUE,
  name          TEXT NOT NULL DEFAULT '',   -- set by the user; never by discovery
  reported_name TEXT NOT NULL DEFAULT '',   -- what the device calls itself
  trust         TEXT NOT NULL,
  revision      INTEGER NOT NULL,
  created_ms    INTEGER NOT NULL,
  updated_ms    INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE device_bindings (
  id             TEXT PRIMARY KEY,
  device_id      TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
  instance_id    TEXT NOT NULL,
  remote_context BLOB NOT NULL DEFAULT x'',  -- repository/account/peer context the authorization was given for
  state          TEXT NOT NULL,
  sync_enabled   INTEGER NOT NULL DEFAULT 1,
  revision       INTEGER NOT NULL,
  created_ms     INTEGER NOT NULL,
  updated_ms     INTEGER NOT NULL,
  last_active_ms INTEGER NOT NULL DEFAULT 0,
  last_error     TEXT NOT NULL DEFAULT '',
  UNIQUE (device_id, instance_id)
) WITHOUT ROWID;`

// Device is a stable device record.
type Device struct {
	ID           string
	PublicKey    []byte
	Name         string
	ReportedName string
	Trust        string
	Revision     int64
	CreatedMs    int64
	UpdatedMs    int64
}

// Binding links a device to one local backend instance.
type Binding struct {
	ID            string
	DeviceID      string
	InstanceID    string
	RemoteContext []byte
	State         string
	SyncEnabled   bool
	Revision      int64
	CreatedMs     int64
	UpdatedMs     int64
	LastActiveMs  int64
	LastError     string
}

const deviceCols = `id, public_key, name, reported_name, trust, revision, created_ms, updated_ms`

func scanDevice(r interface{ Scan(...any) error }) (Device, error) {
	var d Device
	err := r.Scan(&d.ID, &d.PublicKey, &d.Name, &d.ReportedName, &d.Trust, &d.Revision, &d.CreatedMs, &d.UpdatedMs)
	return d, err
}

// AddDevice registers a verified device as pending. A known device is returned
// unchanged, except that a revoked one goes back to pending (it needs a new
// approval) and its reported name is refreshed. The same id with another key,
// or the same key under another id, is ErrIdentityConflict and changes nothing.
func (s *Store) AddDevice(id string, publicKey []byte, reportedName string) (Device, error) {
	tx, err := s.db.Begin()
	if err != nil {
		return Device{}, err
	}
	defer tx.Rollback()
	now := time.Now().UnixMilli()
	d, err := scanDevice(tx.QueryRow(`SELECT `+deviceCols+` FROM devices WHERE id = ?`, id))
	switch {
	case errors.Is(err, sql.ErrNoRows):
		var other string
		if e := tx.QueryRow(`SELECT id FROM devices WHERE public_key = ?`, publicKey).Scan(&other); e == nil {
			return Device{}, ErrIdentityConflict
		}
		d = Device{ID: id, PublicKey: publicKey, ReportedName: reportedName, Trust: TrustPending, Revision: 1, CreatedMs: now, UpdatedMs: now}
		if _, err := tx.Exec(`INSERT INTO devices (`+deviceCols+`) VALUES (?,?,?,?,?,?,?,?)`,
			d.ID, d.PublicKey, "", d.ReportedName, d.Trust, d.Revision, d.CreatedMs, d.UpdatedMs); err != nil {
			return Device{}, err
		}
	case err != nil:
		return Device{}, err
	default:
		if !bytes.Equal(d.PublicKey, publicKey) {
			return Device{}, ErrIdentityConflict
		}
		if d.Trust == TrustRevoked || d.ReportedName != reportedName {
			if d.Trust == TrustRevoked {
				d.Trust = TrustPending
			}
			d.ReportedName, d.Revision, d.UpdatedMs = reportedName, d.Revision+1, now
			if _, err := tx.Exec(`UPDATE devices SET trust=?, reported_name=?, revision=?, updated_ms=? WHERE id=?`,
				d.Trust, d.ReportedName, d.Revision, d.UpdatedMs, d.ID); err != nil {
				return Device{}, err
			}
		}
	}
	return d, tx.Commit()
}

func (s *Store) Device(id string) (Device, error) {
	d, err := scanDevice(s.db.QueryRow(`SELECT `+deviceCols+` FROM devices WHERE id = ?`, id))
	if errors.Is(err, sql.ErrNoRows) {
		return Device{}, ErrDeviceNotFound
	}
	return d, err
}

// Devices lists every device except revoked tombstones, by creation order.
func (s *Store) Devices() ([]Device, error) {
	rows, err := s.db.Query(`SELECT ` + deviceCols + ` FROM devices WHERE trust <> 'revoked' ORDER BY created_ms, id`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []Device
	for rows.Next() {
		d, err := scanDevice(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, d)
	}
	return out, rows.Err()
}

func (s *Store) bumpDevice(id string, expected int64, set string, args ...any) (Device, error) {
	now := time.Now().UnixMilli()
	res, err := s.db.Exec(`UPDATE devices SET `+set+`, revision = revision + 1, updated_ms = ? WHERE id = ? AND revision = ?`,
		append(args, now, id, expected)...)
	if err != nil {
		return Device{}, err
	}
	if n, _ := res.RowsAffected(); n == 0 {
		if _, err := s.Device(id); err != nil {
			return Device{}, err
		}
		return Device{}, ErrDeviceConflict
	}
	return s.Device(id)
}

// SetDeviceName sets the user's name for a device ("" clears it), by CAS.
func (s *Store) SetDeviceName(id string, expected int64, name string) (Device, error) {
	return s.bumpDevice(id, expected, `name = ?`, name)
}

// TrustDevice marks a pending device trusted, by CAS. A revoked device cannot
// be trusted; it must first come back as pending through AddDevice.
func (s *Store) TrustDevice(id string, expected int64) (Device, error) {
	d, err := s.Device(id)
	if err != nil {
		return Device{}, err
	}
	if d.Trust == TrustRevoked {
		return Device{}, ErrDeviceNotFound
	}
	return s.bumpDevice(id, expected, `trust = 'trusted'`)
}

// RevokeDevice drops trust and every binding of the device in one transaction,
// by CAS, and leaves a tombstone so the key stays bound to the id.
func (s *Store) RevokeDevice(id string, expected int64) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	res, err := tx.Exec(`UPDATE devices SET trust='revoked', revision=revision+1, updated_ms=? WHERE id=? AND revision=?`,
		time.Now().UnixMilli(), id, expected)
	if err != nil {
		return err
	}
	if n, _ := res.RowsAffected(); n == 0 {
		var one int
		if e := tx.QueryRow(`SELECT 1 FROM devices WHERE id=?`, id).Scan(&one); e != nil {
			return ErrDeviceNotFound
		}
		return ErrDeviceConflict
	}
	if _, err := tx.Exec(`DELETE FROM device_bindings WHERE device_id=?`, id); err != nil {
		return err
	}
	return tx.Commit()
}

const bindingCols = `id, device_id, instance_id, remote_context, state, sync_enabled, revision, created_ms, updated_ms, last_active_ms, last_error`

func scanBinding(r interface{ Scan(...any) error }) (Binding, error) {
	var b Binding
	var en int
	err := r.Scan(&b.ID, &b.DeviceID, &b.InstanceID, &b.RemoteContext, &b.State, &en, &b.Revision, &b.CreatedMs, &b.UpdatedMs, &b.LastActiveMs, &b.LastError)
	b.SyncEnabled = en != 0
	return b, err
}

// AddBinding creates a pending binding of a device to an instance. An existing
// binding for the pair is returned as is when its remote context is the same;
// a different context means the instance was redirected, so the old
// authorization is dropped and the binding restarts as pending.
func (s *Store) AddBinding(id, deviceID, instanceID string, remoteContext []byte) (Binding, error) {
	tx, err := s.db.Begin()
	if err != nil {
		return Binding{}, err
	}
	defer tx.Rollback()
	now := time.Now().UnixMilli()
	var one int
	if err := tx.QueryRow(`SELECT 1 FROM devices WHERE id=? AND trust <> 'revoked'`, deviceID).Scan(&one); err != nil {
		return Binding{}, ErrDeviceNotFound
	}
	b, err := scanBinding(tx.QueryRow(`SELECT `+bindingCols+` FROM device_bindings WHERE device_id=? AND instance_id=?`, deviceID, instanceID))
	switch {
	case errors.Is(err, sql.ErrNoRows):
		b = Binding{ID: id, DeviceID: deviceID, InstanceID: instanceID, RemoteContext: remoteContext, State: BindingPending,
			SyncEnabled: true, Revision: 1, CreatedMs: now, UpdatedMs: now}
		if _, err := tx.Exec(`INSERT INTO device_bindings (`+bindingCols+`) VALUES (?,?,?,?,?,?,?,?,?,?,?)`,
			b.ID, b.DeviceID, b.InstanceID, b.RemoteContext, b.State, 1, b.Revision, b.CreatedMs, b.UpdatedMs, 0, ""); err != nil {
			return Binding{}, err
		}
	case err != nil:
		return Binding{}, err
	case !bytes.Equal(b.RemoteContext, remoteContext):
		b.RemoteContext, b.State, b.SyncEnabled, b.Revision, b.UpdatedMs, b.LastError = remoteContext, BindingPending, true, b.Revision+1, now, ""
		if _, err := tx.Exec(`UPDATE device_bindings SET remote_context=?, state=?, sync_enabled=1, revision=?, updated_ms=?, last_error='' WHERE id=?`,
			b.RemoteContext, b.State, b.Revision, b.UpdatedMs, b.ID); err != nil {
			return Binding{}, err
		}
	}
	return b, tx.Commit()
}

func (s *Store) Binding(id string) (Binding, error) {
	b, err := scanBinding(s.db.QueryRow(`SELECT `+bindingCols+` FROM device_bindings WHERE id=?`, id))
	if errors.Is(err, sql.ErrNoRows) {
		return Binding{}, ErrBindingNotFound
	}
	return b, err
}

// Bindings lists the bindings of one device.
func (s *Store) Bindings(deviceID string) ([]Binding, error) {
	rows, err := s.db.Query(`SELECT `+bindingCols+` FROM device_bindings WHERE device_id=? ORDER BY created_ms, id`, deviceID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []Binding
	for rows.Next() {
		b, err := scanBinding(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, b)
	}
	return out, rows.Err()
}

func (s *Store) bumpBinding(id string, expected int64, set string, args ...any) (Binding, error) {
	res, err := s.db.Exec(`UPDATE device_bindings SET `+set+`, revision = revision + 1, updated_ms = ? WHERE id = ? AND revision = ?`,
		append(args, time.Now().UnixMilli(), id, expected)...)
	if err != nil {
		return Binding{}, err
	}
	if n, _ := res.RowsAffected(); n == 0 {
		if _, err := s.Binding(id); err != nil {
			return Binding{}, err
		}
		return Binding{}, ErrBindingConflict
	}
	return s.Binding(id)
}

// AuthorizeBinding grants a pending binding, by CAS. It needs a trusted device.
func (s *Store) AuthorizeBinding(id string, expected int64) (Binding, error) {
	b, err := s.Binding(id)
	if err != nil {
		return Binding{}, err
	}
	d, err := s.Device(b.DeviceID)
	if err != nil || d.Trust != TrustTrusted {
		return Binding{}, fmt.Errorf("store: device is not trusted: %w", ErrDeviceNotFound)
	}
	return s.bumpBinding(id, expected, `state = 'authorized'`)
}

// SetBindingSync pauses or resumes content sync on one path, by CAS.
func (s *Store) SetBindingSync(id string, expected int64, enabled bool) (Binding, error) {
	v := 0
	if enabled {
		v = 1
	}
	return s.bumpBinding(id, expected, `sync_enabled = ?`, v)
}

// ReportBinding records activity and a stable, redacted error class. It is a
// status update, not an authorization change, so it takes no revision check
// and does not bump the revision.
func (s *Store) ReportBinding(id string, lastActiveMs int64, lastError string) error {
	res, err := s.db.Exec(`UPDATE device_bindings SET last_active_ms=?, last_error=? WHERE id=?`, lastActiveMs, lastError, id)
	if err != nil {
		return err
	}
	if n, _ := res.RowsAffected(); n == 0 {
		return ErrBindingNotFound
	}
	return nil
}

// RemoveBinding revokes one path, by CAS. The device and its other paths stay.
func (s *Store) RemoveBinding(id string, expected int64) error {
	res, err := s.db.Exec(`DELETE FROM device_bindings WHERE id=? AND revision=?`, id, expected)
	if err != nil {
		return err
	}
	if n, _ := res.RowsAffected(); n == 0 {
		if _, err := s.Binding(id); err != nil {
			return err
		}
		return ErrBindingConflict
	}
	return nil
}
