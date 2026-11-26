package engine

import (
	"context"
	"errors"
	"net"
	"time"

	"github.com/uniclipboard/unic/plugin/rpc"
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

func receiveFailureOutcome(err error) (DiagnosticResult, Code) {
	var ee *Error
	if errors.As(err, &ee) && ee != nil {
		if ee.Code == CodeCancelled {
			return "cancelled", CodeCancelled
		}
		if ee.Code == CodeLimitExceeded || ee.Code == CodeResourceExhausted || ee.Code == CodePermissionDenied {
			return "rejected", ee.Code
		}
		return "failed", ee.Code
	}
	var ne net.Error
	var re *rpc.Error
	switch {
	case err == nil:
		return "failed", CodeUnavailable
	case errors.Is(err, context.Canceled):
		return "cancelled", CodeCancelled
	case errors.Is(err, context.DeadlineExceeded), errors.As(err, &ne) && ne.Timeout():
		return "failed", CodeTimeout
	case errors.As(err, &re):
		switch re.Code {
		case rpc.CodeCancelled:
			return "cancelled", CodeCancelled
		case rpc.CodeTimeout:
			return "failed", CodeTimeout
		case rpc.CodeInvalidArgument:
			return "failed", CodeInvalidArgument
		case rpc.CodeInvalidState:
			return "failed", CodeInvalidState
		case rpc.CodeLimitExceeded:
			return "rejected", CodeLimitExceeded
		case rpc.CodeExhausted:
			return "rejected", CodeResourceExhausted
		case rpc.CodePermission:
			return "rejected", CodePermissionDenied
		case rpc.CodeInternal:
			return "failed", CodeInternal
		default:
			return "failed", CodeUnavailable
		}
	default:
		return "failed", CodeUnavailable
	}
}

func (e *Engine) logPipelineFailure(ctx context.Context, stage DiagnosticStage, result DiagnosticResult, backend DiagnosticBackend, kind DiagnosticContentKind, code Code) {
	if e.log == nil {
		return
	}
	if code == "" {
		code = CodeInternal
	}
	e.log.WarnContext(ctx, "pipeline.failed",
		"stage", string(stage),
		"result", string(result),
		"backend", string(backend),
		"content_kind", string(kind),
		"code", string(code),
	)
}

func (e *Engine) claimApplyDiagnosticLocked(t *applyTicket, result DiagnosticResult, code Code) (*DiagnosticRecord, func(TraceSpanFinish)) {
	if t.diagnosticRecorded {
		return nil, nil
	}
	t.diagnosticRecorded = true
	endTrace := t.traceEnd
	t.traceEnd = nil
	end := t.ended
	if end.IsZero() {
		end = time.Now()
	}
	return &DiagnosticRecord{Stage: "apply", Result: result, Backend: "none", ContentKind: t.kind, Code: code, Duration: end.Sub(t.started)}, endTrace
}
func (e *Engine) attachApplyTrace(t *applyTicket, kind DiagnosticContentKind, end func(TraceSpanFinish)) {
	if end == nil {
		return
	}
	e.mu.Lock()
	alreadyRecorded := t.diagnosticRecorded
	if !alreadyRecorded {
		t.traceEnd = end
	}
	e.mu.Unlock()
	if alreadyRecorded {
		end(TraceSpanFinish{Result: "cancelled", Code: CodeCancelled, ContentKind: kind})
	}
}
func (e *Engine) emitCancelledApply(ctx context.Context) {
	e.mu.Lock()
	var r *DiagnosticRecord
	var endTrace func(TraceSpanFinish)
	if t := e.ticket; t != nil && t.dead {
		r, endTrace = e.claimApplyDiagnosticLocked(t, "cancelled", CodeCancelled)
	}
	e.mu.Unlock()
	if endTrace != nil && r != nil {
		endTrace(TraceSpanFinish{Result: r.Result, Code: r.Code, ContentKind: r.ContentKind})
	}
	if r != nil {
		e.diagnostic(ctx, *r)
	}
}
