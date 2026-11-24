package mobile

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/v1migration"
)

// SecretProvider supplies platform keychain/keystore v1 credential bytes upon explicit migration start.
type SecretProvider interface {
	ReadSecret(service string, account string) ([]byte, error)
}

// MigrationProgressSink receives per-stage progress updates during offline import and verification.
type MigrationProgressSink interface {
	OnProgress(stage string, processed int, total int)
}

type setupMarker struct {
	Complete bool   `json:"complete"`
	Source   string `json:"source,omitempty"`
}

// MigrationSession manages mobile first-install setup state and offline v1 history migration.
type MigrationSession struct {
	mu            sync.Mutex
	targetDir     string
	defaultSource string
	state         setupMarker
}

// OpenSetupFlow opens or initializes the setup state for targetDir, preserving established v2 installations.
func OpenSetupFlow(targetDir string, defaultSourceDir string) (*MigrationSession, error) {
	resolvedTarget, err := resolveDirPath(targetDir)
	if err != nil {
		return nil, wrapErr(err)
	}
	resolvedDefault := strings.TrimSpace(defaultSourceDir)
	if resolvedDefault != "" {
		if r, rerr := resolveDirPath(resolvedDefault); rerr == nil {
			resolvedDefault = r
		}
	}

	m := &MigrationSession{
		targetDir:     resolvedTarget,
		defaultSource: resolvedDefault,
	}
	raw, err := os.ReadFile(m.markerPath())
	if err == nil {
		if uerr := json.Unmarshal(raw, &m.state); uerr != nil {
			return nil, wrapErr(uerr)
		}
		return m, nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return nil, wrapErr(err)
	}

	// Absence of the setup marker must not treat an existing non-empty v2 database as a fresh install.
	if st, serr := os.Stat(filepath.Join(resolvedTarget, "unic.db")); serr == nil {
		if !st.Mode().IsRegular() || st.Size() == 0 {
			return nil, wrapErr(errors.New("invalid existing v2 database"))
		}
		m.state.Complete = true
	} else if !errors.Is(serr, os.ErrNotExist) {
		return nil, wrapErr(serr)
	}

	if err := m.save(); err != nil {
		return nil, wrapErr(err)
	}
	return m, nil
}

func (m *MigrationSession) markerPath() string {
	return filepath.Join(m.targetDir, "mobile-setup.json")
}

func (m *MigrationSession) save() error {
	if err := os.MkdirAll(m.targetDir, 0o700); err != nil {
		return err
	}
	raw, err := json.Marshal(m.state)
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(m.targetDir, ".mobile-setup-*")
	if err != nil {
		return err
	}
	tmpName := tmp.Name()
	defer func() { _ = os.Remove(tmpName) }()
	if _, err := tmp.Write(append(raw, '\n')); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmpName, m.markerPath())
}

func (m *MigrationSession) updateState(next setupMarker) error {
	prev := m.state
	m.state = next
	if err := m.save(); err != nil {
		m.state = prev
		return err
	}
	return nil
}

// GetState returns {"complete", "source", "default_source"} JSON.
func (m *MigrationSession) GetState() (string, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	return marshalJSON(map[string]any{
		"complete":       m.state.Complete,
		"source":         m.state.Source,
		"default_source": m.defaultSource,
	})
}

func checkCandidateSource(candidate string, targetDir string) (status string, resolved string, err error) {
	if strings.TrimSpace(candidate) == "" {
		return "none", "", nil
	}
	abs, aerr := resolveDirPath(candidate)
	if aerr != nil {
		return "invalid", "", nil
	}
	st, serr := os.Stat(abs)
	if errors.Is(serr, os.ErrNotExist) {
		return "none", "", nil
	}
	if serr != nil {
		return "error", "", serr
	}
	if !st.IsDir() {
		return "invalid", "", nil
	}
	slot, kerr := os.Stat(filepath.Join(abs, "vault", "keyslot.json"))
	if kerr != nil || !slot.Mode().IsRegular() {
		return "invalid", "", nil
	}
	if verr := v1migration.ValidateDirectories(abs, targetDir); verr != nil {
		return "invalid", "", nil
	}
	return "found", abs, nil
}

// DetectSource inspects dir (or the remembered/default source when empty) without accessing credentials or clipboard.
func (m *MigrationSession) DetectSource(dir string) (string, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	candidate := strings.TrimSpace(dir)
	explicit := candidate != ""
	if !explicit {
		if m.state.Source != "" {
			st, _, _ := checkCandidateSource(m.state.Source, m.targetDir)
			if st == "found" {
				candidate = m.state.Source
			} else {
				candidate = m.defaultSource
			}
		} else {
			candidate = m.defaultSource
		}
	}
	status, resolved, err := checkCandidateSource(candidate, m.targetDir)
	if err != nil {
		return "", wrapErr(err)
	}
	if !explicit && status == "invalid" {
		if _, verr := os.Stat(filepath.Join(candidate, "vault")); errors.Is(verr, os.ErrNotExist) {
			status = "none"
			resolved = ""
		}
	}
	if status == "found" && (explicit || m.state.Source == "") {
		next := m.state
		next.Source = resolved
		if uerr := m.updateState(next); uerr != nil {
			return "", wrapErr(uerr)
		}
	}
	return marshalJSON(map[string]string{
		"status": status,
		"source": resolved,
	})
}

// SelectSource validates a user-selected directory and persists it for migration.
func (m *MigrationSession) SelectSource(sourceDir string) (string, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	status, resolved, err := checkCandidateSource(sourceDir, m.targetDir)
	if err != nil {
		return "", wrapErr(err)
	}
	if status != "found" {
		return "", wrapErr(v1migration.ErrFormat)
	}
	next := m.state
	next.Source = resolved
	if uerr := m.updateState(next); uerr != nil {
		return "", wrapErr(uerr)
	}
	return marshalJSON(map[string]string{"source": resolved})
}

// BeginLaterMigration marks setup as pending so the host opens migration even on an established profile.
func (m *MigrationSession) BeginLaterMigration() error {
	m.mu.Lock()
	defer m.mu.Unlock()
	next := m.state
	next.Complete = false
	return wrapErr(m.updateState(next))
}

// MarkComplete persists setup completion when the user explicitly enters history or skips migration.
func (m *MigrationSession) MarkComplete() error {
	m.mu.Lock()
	defer m.mu.Unlock()
	next := m.state
	next.Complete = true
	return wrapErr(m.updateState(next))
}

type migrationRunResult struct {
	v1migration.Report
	Error *mobileError `json:"error,omitempty"`
}

// Run performs offline v1 migration under exclusive DataDir ownership.
func (m *MigrationSession) Run(token *CancelToken, passphrase []byte, secrets SecretProvider, sink MigrationProgressSink) (string, error) {
	ctx, cancel := withToken(token)
	defer cancel()

	m.mu.Lock()
	sourceDir := m.state.Source
	targetDir := m.targetDir
	m.mu.Unlock()

	if err := ctx.Err(); err != nil {
		return "", wrapErr(err)
	}
	if err := v1migration.ValidateDirectories(sourceDir, targetDir); err != nil {
		return "", wrapErr(err)
	}

	// Probe exclusive target ownership BEFORE touching system credentials or prompting biometrics.
	if err := os.MkdirAll(targetDir, 0o700); err != nil {
		return "", wrapErr(err)
	}
	targetDir, _ = resolveDirPath(targetDir)
	probe, err := engine.New(ctx, engine.Options{DataDir: targetDir, MigrationMode: true})
	if err != nil {
		var ee *engine.Error
		if errors.As(err, &ee) && ee.Code == engine.CodeInUse {
			return "", wrapErr(errTargetBusy)
		}
		return "", wrapErr(err)
	}
	if err := probe.Close(context.Background()); err != nil {
		return "", wrapErr(err)
	}

	var pwCopy []byte
	if len(passphrase) > 0 {
		pwCopy = append([]byte(nil), passphrase...)
		defer clear(pwCopy)
	}

	var reader v1migration.SecretReader
	if secrets != nil {
		reader = func(service, account string) ([]byte, error) {
			return secrets.ReadSecret(service, account)
		}
	}

	src, err := v1migration.Open(ctx, sourceDir, "UniClipboard", pwCopy, reader)
	if err != nil {
		return "", wrapErr(err)
	}
	defer func() { _ = src.Close() }()

	report, runErr := src.Run(ctx, v1migration.RunOptions{
		Target: targetDir,
		Progress: func(p v1migration.Progress) {
			if sink != nil {
				sink.OnProgress(p.Stage, p.Processed, p.Total)
			}
		},
	})
	if runErr != nil {
		var ee *engine.Error
		if errors.As(runErr, &ee) && ee.Code == engine.CodeInUse {
			return "", wrapErr(errTargetBusy)
		}
		if report.Total > 0 || ctx.Err() != nil {
			res := migrationRunResult{Report: report}
			if ctx.Err() == nil {
				res.Error = toMobileError(runErr)
			} else {
				res.Status = "cancelled"
			}
			return marshalJSON(res)
		}
		return "", wrapErr(runErr)
	}
	return marshalJSON(migrationRunResult{Report: report})
}
