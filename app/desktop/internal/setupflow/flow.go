// Package setupflow owns first-install persistence and offline v1 migration.
// It has no graphical dependency and never starts a service or clipboard adapter.
package setupflow

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/climigration"
	"github.com/uniclipboard/unic/internal/servicelaunch"
	"github.com/uniclipboard/unic/internal/v1migration"
)

var ErrTargetBusy = errors.New("migration target is in use")
var ErrSetupRequired = errors.New("profile setup is required; run unic-desktop to finish setup or unic migrate to initialize")
var ErrMigrationPending = errors.New("v1 migration is pending; run unic-desktop or unic migrate to finish migration")

// CheckAdmission checks whether target is initialized and ready for standalone
// Desktop activation (such as unic-desktop --quick-panel) without creating
// directories or writing marker files.
func CheckAdmission(target string) error {
	target, err := filepath.Abs(target)
	if err != nil {
		return err
	}
	st, err := os.Stat(target)
	if errors.Is(err, os.ErrNotExist) {
		return ErrSetupRequired
	}
	if err != nil {
		return err
	}
	if !st.IsDir() {
		return errors.New("invalid profile directory")
	}
	cliState, err := climigration.Read(target)
	if err != nil {
		return err
	}
	if cliState.Status == "pending" {
		return ErrMigrationPending
	}
	raw, err := os.ReadFile(filepath.Join(target, "desktop-setup.json"))
	if err == nil {
		var state State
		if err = json.Unmarshal(raw, &state); err != nil {
			return errors.New("invalid desktop setup state")
		}
		if !state.Complete {
			return ErrSetupRequired
		}
		return nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if cliState.Status == "complete" {
		return nil
	}
	if dbStat, dbErr := os.Stat(filepath.Join(target, "unic.db")); dbErr == nil {
		if !dbStat.Mode().IsRegular() || dbStat.Size() == 0 {
			return errors.New("invalid existing v2 database")
		}
		return nil
	} else if !errors.Is(dbErr, os.ErrNotExist) {
		return dbErr
	}
	return ErrSetupRequired
}

// State contains navigation intent only. Secrets and migration records stay out.
type State struct {
	Complete bool   `json:"complete"`
	Source   string `json:"source,omitempty"`
}

type Flow struct {
	mu     sync.Mutex
	target string
	state  State
}

// Open distinguishes an established v2 database from an unfinished first run.
// Persist intent before an importer can create a database for the first time.
func Open(target string) (*Flow, error) {
	target, err := filepath.Abs(target)
	if err != nil {
		return nil, err
	}
	f := &Flow{target: target}
	raw, err := os.ReadFile(f.path())
	if err == nil {
		if err = json.Unmarshal(raw, &f.state); err != nil {
			return nil, err
		}
		return f, nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	if st, e := os.Stat(filepath.Join(target, "unic.db")); e == nil {
		if !st.Mode().IsRegular() || st.Size() == 0 {
			return nil, errors.New("invalid existing v2 database")
		}
		f.state.Complete = true
	} else if !errors.Is(e, os.ErrNotExist) {
		return nil, e
	}
	if err = f.save(); err != nil {
		return nil, err
	}
	return f, nil
}
func (f *Flow) path() string   { return filepath.Join(f.target, "desktop-setup.json") }
func (f *Flow) State() State   { f.mu.Lock(); defer f.mu.Unlock(); return f.state }
func (f *Flow) Required() bool { return !f.State().Complete }

func (f *Flow) SelectSource(source string) error {
	found, err := Detect(source)
	if err != nil {
		return err
	}
	if !found {
		return v1migration.ErrFormat
	}
	source, err = filepath.Abs(source)
	if err != nil {
		return err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	next := f.state
	next.Source = source
	return f.update(next)
}
func (f *Flow) Complete() error {
	f.mu.Lock()
	defer f.mu.Unlock()
	next := f.state
	next.Complete = true
	return f.update(next)
}

// BeginMigration persists pending intent for a later import before the host
// opens the migration screen. Cancellation or process exit must not lose it.
// The host first stops and waits for its normal runtime.
func (f *Flow) BeginMigration() error {
	f.mu.Lock()
	defer f.mu.Unlock()
	next := f.state
	next.Complete = false
	return f.update(next)
}

func (f *Flow) update(next State) error {
	previous := f.state
	f.state = next
	if err := f.save(); err != nil {
		f.state = previous
		return err
	}
	return nil
}
func (f *Flow) save() error {
	if err := os.MkdirAll(f.target, 0700); err != nil {
		return err
	}
	raw, err := json.Marshal(f.state)
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(f.target, ".desktop-setup-*")
	if err != nil {
		return err
	}
	// A successful rename already removed the temporary pathname.
	defer func() { _ = os.Remove(tmp.Name()) }()
	_, err = tmp.Write(append(raw, '\n'))
	if err == nil {
		err = tmp.Sync()
	}
	closeErr := tmp.Close()
	if err != nil {
		return err
	}
	if closeErr != nil {
		return closeErr
	}
	return os.Rename(tmp.Name(), f.path())
}

// Detect checks only public directory structure, never credentials or history.
// Missing directories are not errors. A present but invalid profile is an error.
func Detect(source string) (bool, error) {
	st, err := os.Stat(source)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	if !st.IsDir() {
		return false, v1migration.ErrFormat
	}
	slot, err := os.Stat(filepath.Join(source, "vault", "keyslot.json"))
	if err != nil {
		return false, v1migration.ErrFormat
	}
	if !slot.Mode().IsRegular() {
		return false, v1migration.ErrFormat
	}
	return true, nil
}

// Run is synchronous. The host cancels and waits for it before starting normal
// activity or exiting. A supplied SecretReader is the legacy credential adapter.
func (f *Flow) Run(ctx context.Context, password []byte, secrets v1migration.SecretReader, progress func(v1migration.Progress)) (v1migration.Report, error) {
	failed := v1migration.Report{Status: "failed"}
	if err := ctx.Err(); err != nil {
		failed.Status = "cancelled"
		return failed, err
	}
	if err := v1migration.ValidateDirectories(f.State().Source, f.target); err != nil {
		return failed, err
	}
	unlock, err := servicelaunch.LockContext(ctx, f.target)
	if err != nil {
		return failed, err
	}
	defer unlock()
	pending, err := climigration.Begin(f.target, f.State().Source)
	if err != nil {
		return failed, err
	}
	// Check exclusive ownership before any system credential prompt. The importer
	// re-acquires the Engine lock and remains authoritative if ownership changes.
	probe, err := engine.New(ctx, engine.Options{DataDir: f.target, MigrationMode: true})
	if err != nil {
		var ee *engine.Error
		if errors.As(err, &ee) && ee.Code == engine.CodeInUse {
			return failed, ErrTargetBusy
		}
		return failed, err
	}
	if err = probe.Close(context.Background()); err != nil {
		return failed, err
	}
	source, err := v1migration.Open(ctx, f.State().Source, "UniClipboard", password, secrets)
	if err != nil {
		return failed, err
	}
	// Release the read-only source snapshot after import or cancellation.
	defer func() { _ = source.Close() }()
	if err = ctx.Err(); err != nil {
		failed.Status = "cancelled"
		return failed, err
	}
	if _, err = climigration.Bind(f.target, pending, source.ID()); err != nil {
		return failed, err
	}
	report, err := source.Run(ctx, v1migration.RunOptions{Target: f.target, Progress: progress})
	var ee *engine.Error
	if errors.As(err, &ee) && ee.Code == engine.CodeInUse {
		return report, ErrTargetBusy
	}
	if err == nil && report.Status == "complete" && ctx.Err() == nil {
		if err = climigration.Write(f.target, climigration.State{Status: "complete", Outcome: "imported", SourceID: source.ID()}); err != nil {
			report.Status = "failed"
		}
	}
	return report, err
}
