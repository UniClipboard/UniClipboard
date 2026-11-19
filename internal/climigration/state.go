// Package climigration owns the profile-scoped CLI initialization receipt shared
// by explicit CLI and Desktop imports.
// Database existence and Desktop navigation state never grant CLI permission.
package climigration

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

// State records only initialization intent. It contains no credentials or content.
type State struct {
	Status        string `json:"status"`
	Outcome       string `json:"outcome,omitempty"`
	SourceID      string `json:"source_id,omitempty"`
	KeyslotSHA256 string `json:"keyslot_sha256,omitempty"`
}

const Filename = "cli-migration.json"

// Read fails closed on corrupt or unrecognized state.
func Read(dir string) (State, error) {
	raw, err := os.ReadFile(filepath.Join(dir, Filename))
	if errors.Is(err, os.ErrNotExist) {
		return State{}, nil
	}
	if err != nil {
		return State{}, errors.New("cannot read CLI migration state; check data-directory permissions")
	}
	var state State
	if json.Unmarshal(raw, &state) != nil || (state.Status != "pending" && state.Status != "complete") ||
		(state.Status == "complete" && state.Outcome != "imported" && state.Outcome != "no_source") {
		return State{}, errors.New("invalid CLI migration state; run unic migrate --help and complete an explicit import")
	}
	return state, nil
}

// Required reports whether an explicit successful initialization is missing.
func Required(dir string) error {
	state, err := Read(dir)
	if err != nil {
		return err
	}
	if state.Status != "complete" {
		return &RequiredError{}
	}
	return nil
}

type RequiredError struct{}

func (*RequiredError) Error() string {
	return "Migration initialization is required before using this profile. Run unic migrate --help. Import v1 history with unic migrate --json. If no v1 source exists, explicitly initialize with unic migrate --initialize-empty --json."
}

// Write publishes one receipt atomically. Callers serialize with the
// profile transition lock and close the importer before publishing completion.
func Write(dir string, state State) error {
	if err := os.MkdirAll(dir, 0700); err != nil {
		return err
	}
	raw, err := json.Marshal(state)
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".cli-migration-*")
	if err != nil {
		return err
	}
	defer func() { _ = os.Remove(tmp.Name()) }()
	if _, err = tmp.Write(append(raw, '\n')); err == nil {
		err = tmp.Sync()
	}
	closeErr := tmp.Close()
	if err != nil {
		return err
	}
	if closeErr != nil {
		return closeErr
	}
	if err = os.Rename(tmp.Name(), filepath.Join(dir, Filename)); err != nil {
		return fmt.Errorf("publish CLI migration state: %w", err)
	}
	return nil
}

// ErrSourceMismatch prevents another source from completing a pending import.
var ErrSourceMismatch = errors.New("complete the pending source before importing a different source")

// Begin records a real CLI or Desktop import before credentials. Callers must
// validate directory separation and hold the shared profile transition lock.
func Begin(dir, source string) (State, error) {
	f, err := os.Open(filepath.Join(source, "vault", "keyslot.json"))
	if err != nil {
		return State{}, errors.New("cannot read the v1 key slot")
	}
	raw, err := io.ReadAll(io.LimitReader(f, (8<<20)+1))
	closeErr := f.Close()
	if err != nil || closeErr != nil || len(raw) > 8<<20 {
		return State{}, errors.New("cannot read the v1 key slot")
	}
	sum := sha256.Sum256(raw)
	pending := State{Status: "pending", KeyslotSHA256: hex.EncodeToString(sum[:])}
	previous, readErr := Read(dir)
	if readErr == nil && previous.Status == "pending" {
		if previous.SourceID == "" && previous.KeyslotSHA256 != pending.KeyslotSHA256 {
			return State{}, ErrSourceMismatch
		}
		pending = previous
	}
	return pending, Write(dir, pending)
}

// Bind replaces the pre-unlock fingerprint with the importer's stable identity.
func Bind(dir string, pending State, sourceID string) (State, error) {
	if pending.SourceID != "" && pending.SourceID != sourceID {
		return State{}, ErrSourceMismatch
	}
	pending.SourceID = sourceID
	return pending, Write(dir, pending)
}
