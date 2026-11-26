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
	"github.com/uniclipboard/unic/internal/observability"
)

type mobileDiagnostics struct {
	controller *observability.Controller
	path       string
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

func (d *mobileDiagnostics) close(ctx context.Context) {
	d.control.Lock()
	defer d.control.Unlock()
	d.closed = true
	d.controller.Close(ctx)
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

// ExportRecentDiagnosticLogs returns recent, already-redacted local JSON lines.
func (m *Engine) ExportRecentDiagnosticLogs() string {
	d := m.diagnostics
	d.logsMu.Lock()
	defer d.logsMu.Unlock()
	return strings.Join(d.logs[d.next:], "") + strings.Join(d.logs[:d.next], "")
}
