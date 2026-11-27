package mobile

import (
	"archive/zip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/observability"
)

const (
	maxMobileLogSegmentBytes int64 = 4 << 20
	maxMobileLogExportBytes  int64 = 20 << 20
	mobileLogFlushTimeout          = 500 * time.Millisecond
)

type mobileDiagnostics struct {
	controller *observability.Controller
	path       string
	version    string
	control    sync.Mutex
	closed     bool
	logsMu     sync.Mutex
	logs       [64]string
	next       int
}

func newMobileDiagnostics(dir string) *mobileDiagnostics {
	d := &mobileDiagnostics{path: filepath.Join(dir, "mobile-telemetry.json")}
	cfg := observability.Default()
	if raw, err := os.ReadFile(d.path); !errors.Is(err, os.ErrNotExist) {
		// An unreadable preference must not reactivate uploads for an opted-out profile.
		var enabled *bool
		cfg.Telemetry.Enabled = err == nil && json.Unmarshal(raw, &enabled) == nil && enabled != nil && *enabled
	}
	opts := observability.ReleaseOptions(cfg.Telemetry.Endpoint)
	opts.Dir, opts.Config, opts.ServiceName, opts.LocalOutput = dir, cfg, "unic-mobile", d.appendLog
	d.version = opts.Version
	if d.version == "" {
		d.version = "dev"
	}
	d.controller = observability.New(opts)
	return d
}

func traceOutcome(err error) (engine.DiagnosticResult, engine.Code) {
	if err == nil {
		return "succeeded", ""
	}
	var ee *engine.Error
	if errors.As(err, &ee) && ee != nil {
		if ee.Code == engine.CodeCancelled {
			return "cancelled", engine.CodeCancelled
		}
		return "failed", ee.Code
	}
	if errors.Is(err, context.Canceled) {
		return "cancelled", engine.CodeCancelled
	}
	return "failed", engine.CodeInternal
}

func (d *mobileDiagnostics) appendLog(record []byte) {
	d.logsMu.Lock()
	defer d.logsMu.Unlock()
	d.logs[d.next] = string(record)
	d.next = (d.next + 1) % len(d.logs)
}

func (d *mobileDiagnostics) recentLogs() string {
	d.logsMu.Lock()
	defer d.logsMu.Unlock()
	return strings.Join(d.logs[d.next:], "") + strings.Join(d.logs[:d.next], "")
}

func (d *mobileDiagnostics) close(ctx context.Context) {
	d.control.Lock()
	defer d.control.Unlock()
	d.closed = true
	d.controller.Close(ctx)
}

func (d *mobileDiagnostics) isClosed() bool {
	d.control.Lock()
	defer d.control.Unlock()
	return d.closed
}

// GetTelemetryState returns the host's bounded telemetry status as JSON.
func (m *Engine) GetTelemetryState() (string, error) {
	return marshalJSON(m.diagnostics.controller.Telemetry())
}

// SetTelemetryEnabled persists the upload preference and applies it immediately.
func (m *Engine) SetTelemetryEnabled(enabled bool) (string, error) {
	d := m.diagnostics
	d.control.Lock()
	defer d.control.Unlock()
	if d.closed {
		return "", wrapErr(&engine.Error{Code: engine.CodeInvalidState, Message: "mobile diagnostics closed"})
	}
	if err := saveSettingsAtomic(d.path, enabled); err != nil {
		return "", wrapErr(err)
	}
	cfg := observability.Default()
	cfg.Telemetry.Enabled = enabled
	d.controller.Apply(cfg)
	return m.GetTelemetryState()
}

// GetDiagnosticLogLevelState returns the current local and remote log level state as JSON.
func (m *Engine) GetDiagnosticLogLevelState() (string, error) {
	if m.diagnostics.isClosed() {
		return "", wrapErr(&engine.Error{Code: engine.CodeInvalidState, Message: "mobile diagnostics closed"})
	}
	return marshalJSON(m.diagnostics.controller.Levels())
}

// SetLocalLogLevelOverride sets or resets a temporary local log level override (e.g. 10 minutes of debug).
func (m *Engine) SetLocalLogLevelOverride(level string, durationSeconds int, reset bool) (string, error) {
	d := m.diagnostics
	d.control.Lock()
	defer d.control.Unlock()
	if d.closed {
		return "", wrapErr(&engine.Error{Code: engine.CodeInvalidState, Message: "mobile diagnostics closed"})
	}
	state, err := d.controller.SetLevel("local", level, durationSeconds, reset)
	if err != nil {
		code := engine.CodeInvalidArgument
		if err.Error() == "unavailable" {
			code = engine.CodeUnavailable
		}
		return "", wrapErr(&engine.Error{Code: code, Message: err.Error()})
	}
	return marshalJSON(state)
}

// ExportRecentDiagnosticLogs returns recent, already-redacted local JSON lines.
func (m *Engine) ExportRecentDiagnosticLogs() string {
	return m.diagnostics.recentLogs()
}

type mobileDiagnosticAppMeta struct {
	AppVersion string `json:"app_version,omitempty"`
	AppBuild   string `json:"app_build,omitempty"`
	BundleID   string `json:"bundle_id,omitempty"`
	Platform   string `json:"platform,omitempty"`
	OSVersion  string `json:"os_version,omitempty"`
}

type mobileDiagnosticCapabilities struct {
	Observe      string   `json:"observe"`
	Apply        string   `json:"apply"`
	ReadFiles    bool     `json:"read_files"`
	WriteFiles   bool     `json:"write_files"`
	Restrictions []string `json:"restrictions,omitempty"`
}

type mobileDiagnosticRuntime struct {
	Lifecycle            string                       `json:"lifecycle"`
	RuntimeState         string                       `json:"runtime_state"`
	SyncEnabled          bool                         `json:"sync_enabled"`
	SyncAutomatic        bool                         `json:"sync_automatic"`
	SyncFiles            bool                         `json:"sync_files"`
	SyncOnRestore        bool                         `json:"sync_on_restore"`
	PairedDeviceCount    int                          `json:"paired_device_count"`
	ConnectedDeviceCount int                          `json:"connected_device_count"`
	Capabilities         mobileDiagnosticCapabilities `json:"capabilities"`
}

type mobileDiagnosticFileEntry struct {
	Name   string `json:"name"`
	Bytes  int64  `json:"bytes"`
	SHA256 string `json:"sha256"`
}

type mobileDiagnosticEngineLogsSummary struct {
	Status          string   `json:"status"`
	FlushStatus     string   `json:"flush_status"`
	DiscoveredFiles int      `json:"discovered_files"`
	IncludedFiles   int      `json:"included_files"`
	UnreadableFiles int      `json:"unreadable_files"`
	Warnings        []string `json:"warnings"`
}

type mobileDiagnosticManifest struct {
	FormatVersion int                               `json:"format_version"`
	CreatedAt     time.Time                         `json:"created_at"`
	Service       mobileDiagnosticServiceMeta       `json:"service"`
	App           mobileDiagnosticAppMeta           `json:"app"`
	Runtime       *mobileDiagnosticRuntime          `json:"runtime,omitempty"`
	Diagnostics   mobileDiagnosticControllerState   `json:"diagnostics"`
	EngineLogs    mobileDiagnosticEngineLogsSummary `json:"engine_logs"`
	Files         []mobileDiagnosticFileEntry       `json:"files"`
}

type mobileDiagnosticServiceMeta struct {
	Name    string `json:"name"`
	Version string `json:"version"`
	OS      string `json:"os"`
	Arch    string `json:"arch"`
}

type mobileDiagnosticControllerState struct {
	Telemetry observability.TelemetryState `json:"telemetry"`
	LogLevels observability.LogLevelState  `json:"log_levels"`
}

type mobileDiagnosticArchive struct {
	Directory    string   `json:"directory"`
	Path         string   `json:"path"`
	FileName     string   `json:"file_name"`
	MIME         string   `json:"mime"`
	SizeBytes    int64    `json:"size_bytes"`
	LogFileCount int      `json:"log_file_count"`
	Warnings     []string `json:"warnings"`
}

type selectedMobileLog struct {
	name     string
	info     os.FileInfo
	sequence int64
}

func sanitizeDiagnosticToken(raw string) string {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return ""
	}
	var b strings.Builder
	for _, r := range trimmed {
		if b.Len() >= 64 {
			break
		}
		if unicode.IsControl(r) {
			continue
		}
		b.WriteRune(r)
	}
	return b.String()
}

func parseDiagnosticAppMeta(metaJSON string) (mobileDiagnosticAppMeta, error) {
	var raw mobileDiagnosticAppMeta
	trimmed := strings.TrimSpace(metaJSON)
	if trimmed != "" {
		if err := json.Unmarshal([]byte(trimmed), &raw); err != nil {
			return mobileDiagnosticAppMeta{}, &engine.Error{Code: engine.CodeInvalidArgument, Message: "invalid diagnostic metadata json"}
		}
	}
	return mobileDiagnosticAppMeta{
		AppVersion: sanitizeDiagnosticToken(raw.AppVersion),
		AppBuild:   sanitizeDiagnosticToken(raw.AppBuild),
		BundleID:   sanitizeDiagnosticToken(raw.BundleID),
		Platform:   sanitizeDiagnosticToken(raw.Platform),
		OSVersion:  sanitizeDiagnosticToken(raw.OSVersion),
	}, nil
}

func (m *Engine) safeDiagnosticRuntime() *mobileDiagnosticRuntime {
	st, err := m.e.GetState(context.Background())
	if err != nil {
		return nil
	}
	paired := 0
	connected := 0
	for _, dev := range st.Devices {
		if dev.Trust == "trusted" {
			paired++
		}
		for _, b := range dev.Bindings {
			if b.Connection == engine.Connected {
				connected++
				break
			}
		}
	}
	var restrictions []string
	if len(st.Capabilities.Restrictions) > 0 {
		restrictions = append([]string(nil), st.Capabilities.Restrictions...)
	}
	return &mobileDiagnosticRuntime{
		Lifecycle:            string(st.Lifecycle),
		RuntimeState:         string(st.Runtime),
		SyncEnabled:          st.Settings.Value.Sync.Enabled,
		SyncAutomatic:        st.Settings.Value.Sync.Automatic,
		SyncFiles:            st.Settings.Value.Sync.Files,
		SyncOnRestore:        st.Settings.Value.Sync.OnRestore,
		PairedDeviceCount:    paired,
		ConnectedDeviceCount: connected,
		Capabilities: mobileDiagnosticCapabilities{
			Observe:      string(st.Capabilities.Effective.Observe),
			Apply:        string(st.Capabilities.Effective.Apply),
			ReadFiles:    st.Capabilities.Effective.ReadFiles,
			WriteFiles:   st.Capabilities.Effective.WriteFiles,
			Restrictions: restrictions,
		},
	}
}

// ExportDiagnosticArchive creates a private, redacted ZIP diagnostic bundle
// containing retained on-disk Engine JSONL segments and manifest.json with
// SHA-256 digests and safe runtime/telemetry state.
func (m *Engine) ExportDiagnosticArchive(metaJSON string) (string, error) {
	if m.diagnostics.isClosed() {
		return "", wrapErr(&engine.Error{Code: engine.CodeInvalidState, Message: "mobile diagnostics closed"})
	}
	appMeta, err := parseDiagnosticAppMeta(metaJSON)
	if err != nil {
		return "", wrapErr(err)
	}

	flushCtx, cancel := context.WithTimeout(context.Background(), mobileLogFlushTimeout)
	flushed := m.diagnostics.controller.FlushLocal(flushCtx)
	cancel()
	flushStatus := "completed"
	if !flushed {
		flushStatus = "incomplete"
	}

	exportsRoot := filepath.Join(m.cacheDir, "exports")
	if err := os.MkdirAll(exportsRoot, 0700); err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot prepare diagnostics export directory"})
	}
	pruneExpiredExports(exportsRoot)

	stageDir, err := os.MkdirTemp(exportsRoot, "diagnostics-")
	if err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot create diagnostics staging directory"})
	}
	keep := false
	defer func() {
		if !keep {
			_ = os.RemoveAll(stageDir)
		}
	}()

	now := time.Now().UTC()
	fileName := fmt.Sprintf("unic-diagnostics-%s.zip", now.Format("20060102T150405Z"))
	destRoot, err := os.OpenRoot(stageDir)
	if err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot open diagnostics staging directory"})
	}
	defer func() { _ = destRoot.Close() }()

	archiveFile, err := destRoot.OpenFile(fileName, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot create diagnostics archive"})
	}
	defer func() { _ = archiveFile.Close() }()

	zw := zip.NewWriter(archiveFile)
	files, summary := m.writeEngineLogsToZip(zw, now, flushStatus)

	manifest := mobileDiagnosticManifest{
		FormatVersion: 1,
		CreatedAt:     now,
		Service: mobileDiagnosticServiceMeta{
			Name:    "unic-mobile",
			Version: m.diagnostics.version,
			OS:      runtime.GOOS,
			Arch:    runtime.GOARCH,
		},
		App:     appMeta,
		Runtime: m.safeDiagnosticRuntime(),
		Diagnostics: mobileDiagnosticControllerState{
			Telemetry: m.diagnostics.controller.Telemetry(),
			LogLevels: m.diagnostics.controller.Levels(),
		},
		EngineLogs: summary,
		Files:      files,
	}
	manifestBytes, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		_ = zw.Close()
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot encode diagnostic manifest"})
	}
	manifestBytes = append(manifestBytes, '\n')
	if err := writeZipBytes(zw, "manifest.json", now, manifestBytes); err != nil {
		_ = zw.Close()
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot write diagnostic manifest"})
	}
	if err := zw.Close(); err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot finalize diagnostics archive"})
	}
	if err := archiveFile.Sync(); err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot save diagnostics archive"})
	}
	info, err := archiveFile.Stat()
	if err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot inspect diagnostics archive"})
	}
	if err := archiveFile.Close(); err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "cannot close diagnostics archive"})
	}

	out := mobileDiagnosticArchive{
		Directory:    stageDir,
		Path:         filepath.Join(stageDir, fileName),
		FileName:     fileName,
		MIME:         "application/zip",
		SizeBytes:    info.Size(),
		LogFileCount: len(files),
		Warnings:     summary.Warnings,
	}
	encoded, err := marshalJSON(out)
	if err != nil {
		return "", err
	}
	keep = true
	return encoded, nil
}

func (m *Engine) writeEngineLogsToZip(zw *zip.Writer, now time.Time, flushStatus string) ([]mobileDiagnosticFileEntry, mobileDiagnosticEngineLogsSummary) {
	files := []mobileDiagnosticFileEntry{}
	summary := mobileDiagnosticEngineLogsSummary{
		Status:      "missing",
		FlushStatus: flushStatus,
		Warnings:    []string{},
	}

	base, err := os.OpenRoot(m.dataDir)
	if err == nil {
		defer func() { _ = base.Close() }()
		info, lstatErr := base.Lstat("logs")
		if lstatErr == nil {
			if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
				summary.UnreadableFiles++
			} else if logsRoot, openErr := base.OpenRoot("logs"); openErr != nil {
				summary.UnreadableFiles++
			} else {
				defer func() { _ = logsRoot.Close() }()
				actual, statErr := logsRoot.Stat(".")
				if statErr != nil || !os.SameFile(info, actual) {
					summary.UnreadableFiles++
				} else {
					selected, discovered, unreadable := selectMobileLogs(logsRoot)
					summary.DiscoveredFiles = discovered
					summary.UnreadableFiles += unreadable
					for _, seg := range selected {
						entry, copyErr := copyMobileLogSegmentToZip(zw, logsRoot, seg)
						if copyErr != nil {
							summary.UnreadableFiles++
							continue
						}
						if entry != nil {
							files = append(files, *entry)
						}
					}
					summary.IncludedFiles = len(files)
				}
			}
		} else if !errors.Is(lstatErr, os.ErrNotExist) {
			summary.UnreadableFiles++
		}
	} else {
		summary.UnreadableFiles++
	}

	if len(files) > 0 {
		if summary.UnreadableFiles > 0 {
			summary.Status = "partial"
			summary.Warnings = append(summary.Warnings, "engine_logs_unreadable")
		} else {
			summary.Status = "included"
		}
		return files, summary
	}

	// Fallback: include already-redacted in-memory records when disk logs were unavailable.
	if recent := m.diagnostics.recentLogs(); recent != "" {
		raw := []byte(recent)
		entryName := "logs/engine/recent-memory.jsonl"
		if err := writeZipBytes(zw, entryName, now, raw); err == nil {
			sum := sha256.Sum256(raw)
			files = append(files, mobileDiagnosticFileEntry{
				Name:   entryName,
				Bytes:  int64(len(raw)),
				SHA256: hex.EncodeToString(sum[:]),
			})
			summary.IncludedFiles = 1
			summary.Status = "memory_fallback"
		}
	}

	if summary.UnreadableFiles > 0 {
		summary.Warnings = append(summary.Warnings, "engine_logs_unreadable")
	} else {
		summary.Warnings = append(summary.Warnings, "engine_logs_missing")
	}
	return files, summary
}

func selectMobileLogs(root *os.Root) (selected []selectedMobileLog, discovered int, unreadable int) {
	entries, err := fs.ReadDir(root.FS(), ".")
	if err != nil {
		return nil, 0, 1
	}
	var candidates []selectedMobileLog
	for _, entry := range entries {
		name := entry.Name()
		if !strings.HasPrefix(name, "segment-") || !strings.HasSuffix(name, ".jsonl") {
			continue
		}
		seq, err := strconv.ParseInt(strings.TrimSuffix(strings.TrimPrefix(name, "segment-"), ".jsonl"), 10, 64)
		if err != nil || seq <= 0 {
			continue
		}
		discovered++
		info, err := root.Lstat(name)
		if os.IsNotExist(err) {
			discovered--
			continue
		}
		if err != nil || !info.Mode().IsRegular() {
			unreadable++
			continue
		}
		size := info.Size()
		if size < 0 || size > maxMobileLogSegmentBytes {
			unreadable++
			continue
		}
		candidates = append(candidates, selectedMobileLog{name: name, info: info, sequence: seq})
	}
	sort.Slice(candidates, func(i, j int) bool {
		if candidates[i].sequence == candidates[j].sequence {
			return candidates[i].name < candidates[j].name
		}
		return candidates[i].sequence < candidates[j].sequence
	})
	// Keep newest segments within the 20 MiB total archive budget if over limit.
	var total int64
	start := len(candidates)
	for i := len(candidates) - 1; i >= 0; i-- {
		size := candidates[i].info.Size()
		if size > maxMobileLogExportBytes-total {
			unreadable++
			continue
		}
		total += size
		start = i
	}
	if start < len(candidates) {
		var kept []selectedMobileLog
		total = 0
		for i := len(candidates) - 1; i >= 0; i-- {
			size := candidates[i].info.Size()
			if size > maxMobileLogExportBytes-total {
				continue
			}
			total += size
			kept = append(kept, candidates[i])
		}
		for i, j := 0, len(kept)-1; i < j; i, j = i+1, j-1 {
			kept[i], kept[j] = kept[j], kept[i]
		}
		return kept, discovered, unreadable
	}
	return nil, discovered, unreadable
}

func copyMobileLogSegmentToZip(zw *zip.Writer, root *os.Root, seg selectedMobileLog) (*mobileDiagnosticFileEntry, error) {
	file, err := root.Open(seg.name)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	defer func() { _ = file.Close() }()

	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || !os.SameFile(seg.info, info) {
		return nil, errors.New("segment changed")
	}
	size := seg.info.Size()
	if info.Size() < size {
		return nil, errors.New("segment truncated")
	}
	entryName := "logs/engine/" + seg.name
	hdr := &zip.FileHeader{
		Name:     entryName,
		Method:   zip.Deflate,
		Modified: seg.info.ModTime().UTC().Truncate(time.Second),
	}
	hdr.SetMode(0600)
	writer, err := zw.CreateHeader(hdr)
	if err != nil {
		return nil, err
	}
	hash := sha256.New()
	if _, err := io.CopyN(io.MultiWriter(writer, hash), file, size); err != nil {
		return nil, err
	}
	return &mobileDiagnosticFileEntry{
		Name:   entryName,
		Bytes:  size,
		SHA256: hex.EncodeToString(hash.Sum(nil)),
	}, nil
}

func writeZipBytes(zw *zip.Writer, name string, modTime time.Time, data []byte) error {
	hdr := &zip.FileHeader{
		Name:     name,
		Method:   zip.Deflate,
		Modified: modTime.UTC().Truncate(time.Second),
	}
	hdr.SetMode(0600)
	w, err := zw.CreateHeader(hdr)
	if err != nil {
		return err
	}
	_, err = w.Write(data)
	return err
}
