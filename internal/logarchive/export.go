// Package logarchive exports retained host diagnostics to a private ZIP archive.
package logarchive

import (
	"archive/zip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/releaseinfo"
)

const (
	maxLogSegmentBytes = int64(4 << 20)
	maxLogExportBytes  = int64(20 << 20)
	liveProbeTimeout   = 500 * time.Millisecond
)

type selectedLog struct {
	name     string
	info     os.FileInfo
	sequence int64
}

type exportedLog struct {
	Name   string `json:"name"`
	Bytes  int64  `json:"bytes"`
	SHA256 string `json:"sha256"`
}

type logArchiveServiceMeta struct {
	Name        string `json:"name"`
	Version     string `json:"version"`
	BuildNumber string `json:"build_number,omitempty"`
	OS          string `json:"os"`
	Arch        string `json:"arch"`
}

type logArchiveCapabilities struct {
	Observe      string   `json:"observe"`
	Apply        string   `json:"apply"`
	ReadFiles    bool     `json:"read_files"`
	WriteFiles   bool     `json:"write_files"`
	Restrictions []string `json:"restrictions,omitempty"`
}

type logArchiveRuntime struct {
	SyncEnabled          *bool                   `json:"sync_enabled,omitempty"`
	SyncAutomatic        *bool                   `json:"sync_automatic,omitempty"`
	SyncFiles            *bool                   `json:"sync_files,omitempty"`
	SyncOnRestore        *bool                   `json:"sync_on_restore,omitempty"`
	PairedDeviceCount    *int                    `json:"paired_device_count,omitempty"`
	ConnectedDeviceCount *int                    `json:"connected_device_count,omitempty"`
	PendingDeviceCount   *int                    `json:"pending_device_count,omitempty"`
	Capabilities         *logArchiveCapabilities `json:"capabilities,omitempty"`
}

type logArchiveDiagnostics struct {
	Telemetry *observability.TelemetryState `json:"telemetry,omitempty"`
	LogLevels *observability.LogLevelState  `json:"log_levels,omitempty"`
}

type logArchiveEngineLogs struct {
	Status          string `json:"status"`
	DiscoveredFiles int    `json:"discovered_files"`
	IncludedFiles   int    `json:"included_files"`
}

type logArchiveManifest struct {
	FormatVersion   int                    `json:"format_version"`
	CreatedAt       time.Time              `json:"created_at"`
	Service         logArchiveServiceMeta  `json:"service"`
	ServiceSnapshot string                 `json:"service_snapshot"`
	Runtime         *logArchiveRuntime     `json:"runtime,omitempty"`
	Diagnostics     *logArchiveDiagnostics `json:"diagnostics,omitempty"`
	EngineLogs      logArchiveEngineLogs   `json:"engine_logs"`
	Files           []exportedLog          `json:"files"`
}

// Export copies retained on-disk segments using private rooted handles and
// exclusive creation. The optional client must connect directly to the
// existing unicd socket, without acquiring or launching a service. A nil
// client produces an offline archive.
func Export(ctx context.Context, profile, output, producer string, client *api.Client) (string, error) {
	base, err := os.OpenRoot(profile)
	if err != nil {
		return "", errors.New("cannot open the local profile")
	}
	defer func() { _ = base.Close() }()
	info, err := base.Lstat("logs")
	if err != nil {
		return "", errors.New("local logs are unavailable")
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return "", errors.New("local logs must be a regular directory")
	}
	logs, err := base.OpenRoot("logs")
	if err != nil {
		return "", errors.New("cannot open local logs")
	}
	defer func() { _ = logs.Close() }()
	actual, err := logs.Stat(".")
	if err != nil || !os.SameFile(info, actual) {
		return "", errors.New("local logs directory changed; retry export")
	}
	selected, err := selectLocalLogs(logs)
	if err != nil {
		return "", err
	}
	absolute, err := filepath.Abs(output)
	if err != nil {
		return "", errors.New("invalid archive destination")
	}
	destination, err := os.OpenRoot(filepath.Dir(absolute))
	if err != nil {
		return "", errors.New("archive destination directory is unavailable")
	}
	defer func() { _ = destination.Close() }()
	parent, err := destination.Stat(".")
	if err != nil {
		return "", errors.New("cannot inspect archive destination")
	}
	if os.SameFile(actual, parent) {
		return "", errors.New("archive destination must be outside the managed logs directory")
	}
	name := filepath.Base(absolute)
	file, err := destination.OpenFile(name, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if errors.Is(err, fs.ErrExist) {
		return "", errors.New("archive destination already exists")
	}
	if err != nil {
		return "", errors.New("cannot create log archive")
	}
	owned, statErr := file.Stat()
	if statErr != nil {
		_ = file.Close()
		_ = destination.Remove(name)
		return "", errors.New("cannot inspect new log archive")
	}
	complete := false
	defer func() {
		_ = file.Close()
		if !complete {
			if current, err := destination.Lstat(name); err == nil && os.SameFile(owned, current) {
				_ = destination.Remove(name)
			}
		}
	}()
	if err := writeLogArchive(ctx, file, logs, selected, producer, client); err != nil {
		return "", err
	}
	if err := file.Sync(); err != nil {
		return "", errors.New("cannot save log archive")
	}
	if err := file.Close(); err != nil {
		return "", errors.New("cannot close log archive")
	}
	complete = true
	return absolute, nil
}

func selectLocalLogs(root *os.Root) ([]selectedLog, error) {
	entries, err := fs.ReadDir(root.FS(), ".")
	if err != nil {
		return nil, errors.New("cannot list local logs")
	}
	var selected []selectedLog
	var total int64
	for _, entry := range entries {
		name := entry.Name()
		if !strings.HasPrefix(name, "segment-") || !strings.HasSuffix(name, ".jsonl") {
			continue
		}
		sequence, err := strconv.ParseInt(strings.TrimSuffix(strings.TrimPrefix(name, "segment-"), ".jsonl"), 10, 64)
		if err != nil || sequence <= 0 {
			continue
		}
		info, err := root.Lstat(name)
		if os.IsNotExist(err) {
			continue
		} // retention removed an unopened segment
		if err != nil {
			return nil, errors.New("cannot inspect local log segment")
		}
		if !info.Mode().IsRegular() {
			return nil, errors.New("local log segments must be regular files")
		}
		size := info.Size()
		if size < 0 || size > maxLogSegmentBytes || size > maxLogExportBytes-total {
			return nil, errors.New("local logs exceed archive size limits")
		}
		total += size
		selected = append(selected, selectedLog{name: name, info: info, sequence: sequence})
	}
	if len(selected) == 0 {
		return nil, errors.New("no local log segments to export")
	}
	sort.Slice(selected, func(i, j int) bool {
		if selected[i].sequence == selected[j].sequence {
			return selected[i].name < selected[j].name
		}
		return selected[i].sequence < selected[j].sequence
	})
	return selected, nil
}

// probeLiveDiagnostics performs a bounded, best-effort read-only query against
// an already-running unicd socket. It never starts a temporary service and
// never exports device names, identities, paths, config values or secrets.
func probeLiveDiagnostics(ctx context.Context, client *api.Client) (string, *logArchiveRuntime, *logArchiveDiagnostics) {
	if client == nil {
		return "offline", nil, nil
	}
	probeCtx, cancel := context.WithTimeout(ctx, liveProbeTimeout)
	defer cancel()

	var rt logArchiveRuntime
	hasRuntime := false

	if st, err := client.Status(probeCtx); err == nil {
		paired := 0
		connected := 0
		for _, peer := range st.Peers {
			if peer.Kind == "external" {
				continue
			}
			paired++
			if peer.Connected {
				connected++
			}
		}
		pending := len(st.Pending)
		rt.PairedDeviceCount = &paired
		rt.ConnectedDeviceCount = &connected
		rt.PendingDeviceCount = &pending
		hasRuntime = true
	}
	if caps, err := client.Capabilities(probeCtx); err == nil {
		var restrictions []string
		if len(caps.Restrictions) > 0 {
			restrictions = append([]string(nil), caps.Restrictions...)
		}
		rt.Capabilities = &logArchiveCapabilities{
			Observe:      string(caps.Effective.Observe),
			Apply:        string(caps.Effective.Apply),
			ReadFiles:    caps.Effective.ReadFiles,
			WriteFiles:   caps.Effective.WriteFiles,
			Restrictions: restrictions,
		}
		hasRuntime = true
	}
	if cfg, err := client.Config(probeCtx); err == nil {
		enabled := cfg.Config.Sync.Enabled
		automatic := cfg.Config.Sync.Automatic
		files := cfg.Config.Sync.Files
		onRestore := cfg.Config.Sync.OnRestore
		rt.SyncEnabled = &enabled
		rt.SyncAutomatic = &automatic
		rt.SyncFiles = &files
		rt.SyncOnRestore = &onRestore
		hasRuntime = true
	}

	var diag logArchiveDiagnostics
	hasDiag := false
	if tel, err := client.Telemetry(probeCtx); err == nil {
		diag.Telemetry = &tel
		hasDiag = true
	}
	if levels, err := client.LogLevels(probeCtx); err == nil {
		diag.LogLevels = &levels
		hasDiag = true
	}

	if !hasRuntime && !hasDiag {
		return "offline", nil, nil
	}
	var rtPtr *logArchiveRuntime
	if hasRuntime {
		rtPtr = &rt
	}
	var diagPtr *logArchiveDiagnostics
	if hasDiag {
		diagPtr = &diag
	}
	return "live", rtPtr, diagPtr
}

func writeLogArchive(ctx context.Context, out io.Writer, root *os.Root, selected []selectedLog, producer string, client *api.Client) error {
	archive := zip.NewWriter(out)
	defer func() { _ = archive.Close() }()

	now := time.Now().UTC()
	snapshotState, runtimeSummary, diagSummary := probeLiveDiagnostics(ctx, client)
	manifest := logArchiveManifest{
		FormatVersion: 1,
		CreatedAt:     now,
		Service: logArchiveServiceMeta{
			Name:        producer,
			Version:     releaseinfo.Version,
			BuildNumber: releaseinfo.BuildNumber,
			OS:          runtime.GOOS,
			Arch:        runtime.GOARCH,
		},
		ServiceSnapshot: snapshotState,
		Runtime:         runtimeSummary,
		Diagnostics:     diagSummary,
		EngineLogs: logArchiveEngineLogs{
			Status:          "included",
			DiscoveredFiles: len(selected),
		},
	}
	for _, segment := range selected {
		if err := ctx.Err(); err != nil {
			return errors.New("log export cancelled")
		}
		record, err := copyLogSegment(ctx, archive, root, segment)
		if err != nil {
			return err
		}
		if record != nil {
			manifest.Files = append(manifest.Files, *record)
		}
	}
	if len(manifest.Files) == 0 {
		return errors.New("local log segments were removed; retry export")
	}
	manifest.EngineLogs.IncludedFiles = len(manifest.Files)
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return errors.New("cannot encode log archive manifest")
	}
	data = append(data, '\n')
	hdr := &zip.FileHeader{
		Name:     "manifest.json",
		Method:   zip.Deflate,
		Modified: now.Truncate(time.Second),
	}
	hdr.SetMode(0600)
	writer, err := archive.CreateHeader(hdr)
	if err != nil {
		return errors.New("cannot write log archive manifest")
	}
	if _, err := writer.Write(data); err != nil {
		return errors.New("cannot write log archive manifest")
	}
	if err := archive.Close(); err != nil {
		return errors.New("cannot finish log archive")
	}
	return nil
}

func copyLogSegment(ctx context.Context, archive *zip.Writer, root *os.Root, segment selectedLog) (*exportedLog, error) {
	file, err := root.Open(segment.name)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, errors.New("cannot open local log segment")
	}
	defer func() { _ = file.Close() }()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || !os.SameFile(segment.info, info) {
		return nil, errors.New("local log segment changed; retry export")
	}
	if info.Size() < segment.info.Size() {
		return nil, errors.New("local log segment was truncated; retry export")
	}
	name := "logs/engine/" + segment.name
	size := segment.info.Size()
	hdr := &zip.FileHeader{
		Name:     name,
		Method:   zip.Deflate,
		Modified: segment.info.ModTime().UTC().Truncate(time.Second),
	}
	hdr.SetMode(0600)
	writer, err := archive.CreateHeader(hdr)
	if err != nil {
		return nil, errors.New("cannot write log archive")
	}
	hash := sha256.New()
	if _, err := io.CopyN(io.MultiWriter(writer, hash), logSnapshotReader{ctx: ctx, reader: file}, size); err != nil {
		return nil, errors.New("cannot copy local log segment")
	}
	if err := file.Close(); err != nil {
		return nil, errors.New("cannot close local log segment")
	}
	return &exportedLog{Name: name, Bytes: size, SHA256: hex.EncodeToString(hash.Sum(nil))}, nil
}

type logSnapshotReader struct {
	ctx    context.Context
	reader io.Reader
}

func (r logSnapshotReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.reader.Read(p)
}
