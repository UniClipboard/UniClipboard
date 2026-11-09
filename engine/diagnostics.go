package engine

import (
	"context"
	"errors"
	"time"
)

type DiagnosticStage string
type DiagnosticResult string
type DiagnosticBackend string
type DiagnosticContentKind string

// DiagnosticRecord contains only bounded categories, never content or IDs.
type DiagnosticRecord struct {
	Stage       DiagnosticStage
	Result      DiagnosticResult
	Backend     DiagnosticBackend
	ContentKind DiagnosticContentKind
	Code        Code
	Duration    time.Duration
}

func (e *Engine) diagnostic(ctx context.Context, r DiagnosticRecord) {
	if e.opts.Diagnostics == nil {
		return
	}
	defer func() { _ = recover() }()
	e.opts.Diagnostics(ctx, r)
}
func diagnosticBackend(id PluginID) DiagnosticBackend {
	if id == "" {
		return "none"
	}
	if id == BackendTailcat {
		return "tailcat"
	}
	return "external"
}
func diagnosticKind(c *contentObj) DiagnosticContentKind {
	if c == nil {
		return "unknown"
	}
	var kind DiagnosticContentKind
	for _, p := range c.parts {
		k := DiagnosticContentKind("text")
		if p.desc.Kind == PartFile {
			k = "files"
		} else if len(p.desc.MIME) >= 6 && p.desc.MIME[:6] == "image/" {
			k = "image"
		}
		if kind != "" && kind != k {
			return "mixed"
		}
		kind = k
	}
	if kind == "" {
		return "unknown"
	}
	return kind
}
func diagnosticOutcome(err error) (DiagnosticResult, Code) {
	var ee *Error
	if err == nil || (errors.As(err, &ee) && ee == nil) {
		return "succeeded", ""
	}
	code := asError(err).Code
	if code == CodeCancelled {
		return "cancelled", code
	}
	return "failed", code
}

func (e *Engine) claimApplyDiagnosticLocked(t *applyTicket, result DiagnosticResult, code Code) *DiagnosticRecord {
	if t.diagnosticRecorded {
		return nil
	}
	t.diagnosticRecorded = true
	end := t.ended
	if end.IsZero() {
		end = time.Now()
	}
	return &DiagnosticRecord{Stage: "apply", Result: result, Backend: "none", ContentKind: t.kind, Code: code, Duration: end.Sub(t.started)}
}
func (e *Engine) emitCancelledApply(ctx context.Context) {
	e.mu.Lock()
	var r *DiagnosticRecord
	if t := e.ticket; t != nil && t.dead {
		r = e.claimApplyDiagnosticLocked(t, "cancelled", CodeCancelled)
	}
	e.mu.Unlock()
	if r != nil {
		e.diagnostic(ctx, *r)
	}
}
