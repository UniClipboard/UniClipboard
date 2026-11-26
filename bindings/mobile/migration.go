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
	"github.com/uniclipboard/unic/internal/mobilemigration"
	"github.com/uniclipboard/unic/internal/v1migration"
)

const (
	sourceKindV1Vault      = "v1_vault"
	sourceKindMobileSQLite = "mobile_sqlite"
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
	Complete   bool   `json:"complete"`
	Source     string `json:"source,omitempty"`
	SourceKind string `json:"source_kind,omitempty"`
}

type legacyMobileConfig struct {
	DatabaseCandidates []string `json:"database_candidates"`
	AttachmentRoots    []string `json:"attachment_roots"`
}

// MigrationSession manages mobile first-install setup state and offline v1 / legacy mobile history migration.
type MigrationSession struct {
	mu              sync.Mutex
	targetDir       string
	defaultSource   string
	legacyMobileDBs []string
	attachmentRoots []string
	state           setupMarker
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

// ConfigureLegacyMobile registers platform-specific Expo SQLite database candidates and attachment roots
// used by legacy UniClip mobile releases (v1.3–v2.4) for automatic upgrade detection and migration.
func (m *MigrationSession) ConfigureLegacyMobile(configJSON string) error {
	var cfg legacyMobileConfig
	if strings.TrimSpace(configJSON) != "" {
		if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
			return wrapErr(err)
		}
	}
	resolveList := func(items []string) []string {
		var out []string
		for _, item := range items {
			trimmed := strings.TrimSpace(item)
			if trimmed == "" {
				continue
			}
			if r, err := resolveDirPath(trimmed); err == nil {
				out = append(out, r)
			} else {
				out = append(out, trimmed)
			}
		}
		return out
	}

	m.mu.Lock()
	defer m.mu.Unlock()
	m.legacyMobileDBs = resolveList(cfg.DatabaseCandidates)
	m.attachmentRoots = resolveList(cfg.AttachmentRoots)
	return nil
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

// GetState returns {"complete", "source", "source_kind", "requires_unlock", "default_source"} JSON.
func (m *MigrationSession) GetState() (string, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	kind := m.state.SourceKind
	if kind == "" && m.state.Source != "" {
		if _, _, detectedKind, err := checkCandidateSource(m.state.Source, m.targetDir, true); err == nil {
			kind = detectedKind
		}
	}
	return marshalJSON(map[string]any{
		"complete":        m.state.Complete,
		"source":          m.state.Source,
		"source_kind":     kind,
		"requires_unlock": kind == sourceKindV1Vault,
		"default_source":  m.defaultSource,
	})
}

func checkCandidateSource(candidate string, targetDir string, allowEmptyMobileDB bool) (status string, resolved string, kind string, err error) {
	if strings.TrimSpace(candidate) == "" {
		return "none", "", "", nil
	}
	abs, aerr := resolveDirPath(candidate)
	if aerr != nil {
		return "invalid", "", "", nil
	}
	st, serr := os.Stat(abs)
	if errors.Is(serr, os.ErrNotExist) {
		return "none", "", "", nil
	}
	if serr != nil {
		return "error", "", "", serr
	}

	// 1. Desktop v1 encrypted vault backup (directory containing vault/keyslot.json).
	if st.IsDir() {
		if slot, kerr := os.Stat(filepath.Join(abs, "vault", "keyslot.json")); kerr == nil && slot.Mode().IsRegular() {
			if verr := v1migration.ValidateDirectories(abs, targetDir); verr != nil {
				return "invalid", "", "", nil
			}
			return "found", abs, sourceKindV1Vault, nil
		}
	}

	// 2. Legacy mobile Expo SQLite database (uniclipboard.db file or directory containing it).
	insp, merr := mobilemigration.Inspect(context.Background(), abs)
	if merr == nil {
		if verr := v1migration.ValidateDirectories(filepath.Dir(insp.DatabasePath), targetDir); verr != nil {
			return "invalid", "", "", nil
		}
		if !allowEmptyMobileDB && insp.ActiveCount == 0 {
			return "none", "", "", nil
		}
		return "found", insp.DatabasePath, sourceKindMobileSQLite, nil
	}
	if errors.Is(merr, os.ErrNotExist) {
		return "none", "", "", nil
	}
	return "invalid", "", "", nil
}

// DetectSource inspects dir (or the remembered/legacy mobile/default source when empty)
// without accessing credentials or clipboard.
func (m *MigrationSession) DetectSource(dir string) (string, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	candidate := strings.TrimSpace(dir)
	explicit := candidate != ""

	var (
		status   = "none"
		resolved = ""
		kind     = ""
		err      error
	)

	if explicit {
		status, resolved, kind, err = checkCandidateSource(candidate, m.targetDir, true)
		if err != nil {
			return "", wrapErr(err)
		}
	} else {
		// Priority 1: previously selected valid source.
		if m.state.Source != "" {
			st, res, k, cerr := checkCandidateSource(m.state.Source, m.targetDir, true)
			if cerr == nil && st == "found" {
				status, resolved, kind = st, res, k
			}
		}
		// Priority 2: auto-detected legacy mobile Expo SQLite database with active records.
		if status != "found" {
			for _, dbCand := range m.legacyMobileDBs {
				st, res, k, cerr := checkCandidateSource(dbCand, m.targetDir, false)
				if cerr == nil && st == "found" {
					status, resolved, kind = st, res, k
					break
				}
			}
		}
		// Priority 3: default desktop v1 vault directory.
		if status != "found" && m.defaultSource != "" {
			st, res, k, cerr := checkCandidateSource(m.defaultSource, m.targetDir, false)
			if cerr != nil {
				return "", wrapErr(cerr)
			}
			if st == "invalid" {
				if _, verr := os.Stat(filepath.Join(m.defaultSource, "vault")); errors.Is(verr, os.ErrNotExist) {
					st, res, k = "none", "", ""
				}
			}
			status, resolved, kind = st, res, k
		}
	}

	if status == "found" && (explicit || m.state.Source == "" || m.state.Source != resolved || m.state.SourceKind != kind) {
		next := m.state
		next.Source = resolved
		next.SourceKind = kind
		if uerr := m.updateState(next); uerr != nil {
			return "", wrapErr(uerr)
		}
	}
	return marshalJSON(map[string]any{
		"status":          status,
		"source":          resolved,
		"source_kind":     kind,
		"requires_unlock": kind == sourceKindV1Vault,
	})
}

// SelectSource validates a user-selected directory or legacy SQLite database and persists it for migration.
func (m *MigrationSession) SelectSource(sourceDir string) (string, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	status, resolved, kind, err := checkCandidateSource(sourceDir, m.targetDir, true)
	if err != nil {
		return "", wrapErr(err)
	}
	if status != "found" {
		return "", wrapErr(v1migration.ErrFormat)
	}
	next := m.state
	next.Source = resolved
	next.SourceKind = kind
	if uerr := m.updateState(next); uerr != nil {
		return "", wrapErr(uerr)
	}
	return marshalJSON(map[string]any{
		"source":          resolved,
		"source_kind":     kind,
		"requires_unlock": kind == sourceKindV1Vault,
	})
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

// Run performs offline v1 or legacy mobile SQLite migration under exclusive DataDir ownership.
func (m *MigrationSession) Run(token *CancelToken, passphrase []byte, secrets SecretProvider, sink MigrationProgressSink) (string, error) {
	ctx, cancel := withToken(token)
	defer cancel()

	m.mu.Lock()
	sourceDir := m.state.Source
	sourceKind := m.state.SourceKind
	targetDir := m.targetDir
	attachmentRoots := append([]string(nil), m.attachmentRoots...)
	m.mu.Unlock()

	if err := ctx.Err(); err != nil {
		return "", wrapErr(err)
	}
	if sourceDir == "" {
		return "", wrapErr(v1migration.ErrFormat)
	}
	if sourceKind == "" {
		status, resolved, detectedKind, err := checkCandidateSource(sourceDir, targetDir, true)
		if err != nil {
			return "", wrapErr(err)
		}
		if status != "found" {
			return "", wrapErr(v1migration.ErrFormat)
		}
		sourceDir = resolved
		sourceKind = detectedKind
	}

	validatePath := sourceDir
	if sourceKind == sourceKindMobileSQLite {
		validatePath = filepath.Dir(sourceDir)
	}
	if err := v1migration.ValidateDirectories(validatePath, targetDir); err != nil {
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

	runOpts := v1migration.RunOptions{
		Target: targetDir,
		Progress: func(p v1migration.Progress) {
			if sink != nil {
				sink.OnProgress(p.Stage, p.Processed, p.Total)
			}
		},
	}

	var (
		report v1migration.Report
		runErr error
	)

	if sourceKind == sourceKindMobileSQLite {
		src, oerr := mobilemigration.Open(ctx, sourceDir, mobilemigration.OpenOptions{
			AttachmentRoots: attachmentRoots,
		})
		if oerr != nil {
			return "", wrapErr(oerr)
		}
		defer func() { _ = src.Close() }()
		report, runErr = src.Run(ctx, runOpts)
	} else {
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

		src, oerr := v1migration.Open(ctx, sourceDir, "UniClipboard", pwCopy, reader)
		if oerr != nil {
			return "", wrapErr(oerr)
		}
		defer func() { _ = src.Close() }()
		report, runErr = src.Run(ctx, runOpts)
	}

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
