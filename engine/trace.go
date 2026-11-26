package engine

import "context"

// TraceSpanKind classifies a traced span using standard-library strings only.
type TraceSpanKind string

const (
	TraceSpanInternal TraceSpanKind = "internal"
	TraceSpanClient   TraceSpanKind = "client"
	TraceSpanServer   TraceSpanKind = "server"
	TraceSpanProducer TraceSpanKind = "producer"
	TraceSpanConsumer TraceSpanKind = "consumer"
)

// TraceSpanSpec describes a bounded, allowlisted span started by Engine or its host.
type TraceSpanSpec struct {
	Name         string
	Kind         TraceSpanKind
	RemoteParent string // optional W3C traceparent for inbound cross-device receives
	Backend      DiagnosticBackend
	ContentKind  DiagnosticContentKind
}

// TraceSpanFinish supplies the terminal finite attributes of a span.
type TraceSpanFinish struct {
	Result         DiagnosticResult
	Code           Code
	Backend        DiagnosticBackend
	ContentKind    DiagnosticContentKind
	ActionCategory string
}

// Tracer is an optional host-owned tracing hook using standard-library and
// public Engine types only. Implementations must be non-blocking, panic-safe,
// and never call back into Engine or retain arbitrary caller contexts.
type Tracer interface {
	StartSpan(ctx context.Context, spec TraceSpanSpec) (context.Context, func(TraceSpanFinish))
	InheritTrace(dst, src context.Context) context.Context
	TraceParent(ctx context.Context) string
}

func noopTraceEnd(TraceSpanFinish) {}

func (e *Engine) startSpan(ctx context.Context, spec TraceSpanSpec) (outCtx context.Context, end func(TraceSpanFinish)) {
	if e.opts.Tracer == nil {
		return ctx, noopTraceEnd
	}
	outCtx, end = ctx, noopTraceEnd
	defer func() {
		if recover() != nil {
			outCtx, end = ctx, noopTraceEnd
		}
	}()
	next, finish := e.opts.Tracer.StartSpan(ctx, spec)
	if next != nil {
		outCtx = next
	}
	if finish != nil {
		end = func(f TraceSpanFinish) {
			defer func() { _ = recover() }()
			finish(f)
		}
	}
	return outCtx, end
}

func (e *Engine) inheritTrace(dst, src context.Context) (out context.Context) {
	if e.opts.Tracer == nil || src == nil {
		return dst
	}
	out = dst
	defer func() {
		if recover() != nil {
			out = dst
		}
	}()
	if next := e.opts.Tracer.InheritTrace(dst, src); next != nil {
		out = next
	}
	return out
}

func (e *Engine) traceParent(ctx context.Context) (tp string) {
	if e.opts.Tracer == nil || ctx == nil {
		return ""
	}
	defer func() {
		if recover() != nil {
			tp = ""
		}
	}()
	return e.opts.Tracer.TraceParent(ctx)
}
