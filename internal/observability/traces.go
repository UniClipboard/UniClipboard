package observability

import (
	"context"
	"encoding/hex"
	"errors"
	"net/http"
	"strconv"
	"sync"
	"time"

	"go.opentelemetry.io/otel/attribute"
	otelcodes "go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/trace"

	"github.com/uniclipboard/unic/engine"
)

// Compile-time verification that Controller implements engine.Tracer.
var _ engine.Tracer = (*Controller)(nil)

// Link-time override is reserved for synthetic acceptance builds. There is no
// runtime environment or TOML sampling override. Production roots sample at 1%.
var traceSampleRatio = "0.01"

const traceQueueCapacity = 256
const traceBatchCapacity = 128
const traceExportPeriod = 5 * time.Second

type traceContextKey struct{}
type localTrace struct {
	generation *generation
	parent     trace.SpanContext
}

var allowedSpans = map[string]bool{
	"unic.submission":   true,
	"unic.observe":      true,
	"unic.action":       true,
	"unic.commit":       true,
	"unic.sync.send":    true,
	"unic.sync.receive": true,
	"unic.apply":        true,
}

func (g *generation) initTraces(ctx context.Context, endpoint string, client *http.Client, headers map[string]string) error {
	ratio, err := strconv.ParseFloat(traceSampleRatio, 64)
	if err != nil || !(ratio >= 0 && ratio <= 1) {
		return errors.New("initialization_failed")
	}
	g.spans = make(chan sdktrace.ReadOnlySpan, traceQueueCapacity)
	g.traceExporter, err = otlptracehttp.New(ctx, otlptracehttp.WithEndpointURL(endpoint+"/v1/traces"), otlptracehttp.WithHTTPClient(client), otlptracehttp.WithTLSClientConfig(g.transport.TLSClientConfig), otlptracehttp.WithHeaders(headers), otlptracehttp.WithTimeout(3*time.Second), otlptracehttp.WithRetry(otlptracehttp.RetryConfig{Enabled: false}), otlptracehttp.WithCompression(otlptracehttp.NoCompression))
	if err != nil {
		return err
	}
	limits := sdktrace.NewSpanLimits()
	limits.AttributeCountLimit = 5
	limits.AttributeValueLengthLimit = 64
	// No caller receives an SDK span. Only the fixed allowlisted completion
	// attributes below can be added. Parent context is carried through a
	// private host-only key or sanitized W3C traceparent on unic.sync.receive.
	g.traceProvider = sdktrace.NewTracerProvider(sdktrace.WithResource(g.resource), sdktrace.WithSampler(sdktrace.ParentBased(sdktrace.TraceIDRatioBased(ratio))), sdktrace.WithRawSpanLimits(limits), sdktrace.WithSpanProcessor(g))
	return nil
}

// StartSubmission is called only after unique host submission admission. It
// intentionally ignores caller OTel context, baggage and socket metadata.
func (c *Controller) StartSubmission(ctx context.Context) (context.Context, func(engine.DiagnosticResult, engine.Code)) {
	next, end := c.StartSpan(ctx, engine.TraceSpanSpec{Name: "unic.submission"})
	return next, func(result engine.DiagnosticResult, code engine.Code) {
		end(engine.TraceSpanFinish{Result: result, Code: code})
	}
}

// InheritTrace copies only the local trace generation/parent handle onto dst
// without retaining arbitrary caller context values or cancellation.
func (c *Controller) InheritTrace(dst, src context.Context) context.Context {
	if dst == nil {
		dst = context.Background()
	}
	if src == nil {
		return dst
	}
	if parent, ok := src.Value(traceContextKey{}).(localTrace); ok {
		return context.WithValue(dst, traceContextKey{}, parent)
	}
	return dst
}

// TraceParent returns a W3C Trace Context v00 traceparent only when ctx carries
// an active, sampled span in a live generation. Unsampled traffic returns "".
func (c *Controller) TraceParent(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	lt, ok := ctx.Value(traceContextKey{}).(localTrace)
	if !ok || lt.generation == nil || !lt.parent.IsValid() || !lt.parent.IsSampled() {
		return ""
	}
	lt.generation.mu.Lock()
	active := lt.generation.active
	lt.generation.mu.Unlock()
	if !active {
		return ""
	}
	return "00-" + lt.parent.TraceID().String() + "-" + lt.parent.SpanID().String() + "-01"
}

// StartSpan starts an allowlisted span bound to the current or inherited trace
// generation. Unapproved span names and attributes are ignored.
func (c *Controller) StartSpan(ctx context.Context, spec engine.TraceSpanSpec) (context.Context, func(engine.TraceSpanFinish)) {
	if ctx == nil {
		ctx = context.Background()
	}
	noop := func(engine.TraceSpanFinish) {}
	if !allowedSpans[spec.Name] {
		return ctx, noop
	}
	var g *generation
	var parent trace.SpanContext
	remote := false
	switch {
	case spec.Name == "unic.submission":
		// Always start a fresh local root; ignore caller/socket headers.
		c.mu.Lock()
		g = c.traces
		c.mu.Unlock()
	case spec.Name == "unic.sync.receive" && spec.RemoteParent != "":
		c.mu.Lock()
		g = c.traces
		c.mu.Unlock()
		if sc, ok := parseTraceParent(spec.RemoteParent); ok {
			parent = sc
			remote = true
		}
	default:
		if lt, ok := ctx.Value(traceContextKey{}).(localTrace); ok && lt.generation != nil {
			g = lt.generation
			parent = lt.parent
		} else {
			c.mu.Lock()
			g = c.traces
			c.mu.Unlock()
		}
	}
	if g == nil {
		return ctx, noop
	}
	g.mu.Lock()
	if !g.active {
		g.mu.Unlock()
		return ctx, noop
	}
	clean := context.Background()
	if parent.IsValid() {
		if remote {
			clean = trace.ContextWithRemoteSpanContext(clean, parent)
		} else {
			clean = trace.ContextWithSpanContext(clean, parent)
		}
	}
	_, span := g.traceProvider.Tracer("unic.observability", trace.WithInstrumentationVersion("1")).Start(clean, spec.Name, trace.WithSpanKind(otelSpanKind(spec.Kind)))
	g.mu.Unlock()
	next := context.WithValue(ctx, traceContextKey{}, localTrace{g, span.SpanContext()})
	var once sync.Once
	return next, func(finish engine.TraceSpanFinish) {
		once.Do(func() {
			result, code := normalizeTraceOutcome(finish.Result, finish.Code)
			attrs := make([]attribute.KeyValue, 0, 5)
			attrs = append(attrs, attribute.String("result", string(result)))
			if code != "" {
				attrs = append(attrs, attribute.String("code", string(code)))
			}
			backend := finish.Backend
			if backend == "" {
				backend = spec.Backend
			}
			if validTraceBackend(backend) {
				attrs = append(attrs, attribute.String("backend", string(backend)))
			}
			kind := finish.ContentKind
			if kind == "" {
				kind = spec.ContentKind
			}
			if validTraceContentKind(kind) {
				attrs = append(attrs, attribute.String("content_kind", string(kind)))
			}
			if finish.ActionCategory == "builtin" || finish.ActionCategory == "external" {
				attrs = append(attrs, attribute.String("action_category", finish.ActionCategory))
			}
			span.SetAttributes(attrs...)
			if result == "failed" || result == "rejected" || result == "cancelled" {
				span.SetStatus(otelcodes.Error, "")
			}
			span.End()
		})
	}
}

func otelSpanKind(k engine.TraceSpanKind) trace.SpanKind {
	switch k {
	case engine.TraceSpanClient:
		return trace.SpanKindClient
	case engine.TraceSpanServer:
		return trace.SpanKindServer
	case engine.TraceSpanProducer:
		return trace.SpanKindProducer
	case engine.TraceSpanConsumer:
		return trace.SpanKindConsumer
	default:
		return trace.SpanKindInternal
	}
}

func normalizeTraceOutcome(result engine.DiagnosticResult, code engine.Code) (engine.DiagnosticResult, engine.Code) {
	switch result {
	case "succeeded", "superseded", "duplicate", "policy_filtered":
		return result, ""
	case "cancelled":
		return "cancelled", engine.CodeCancelled
	case "deferred":
		if !validCode(string(code)) {
			code = ""
		}
		return "deferred", code
	case "failed", "rejected":
		if !validCode(string(code)) {
			code = engine.CodeInternal
		}
		return result, code
	default:
		return "failed", engine.CodeInternal
	}
}

func validTraceBackend(b engine.DiagnosticBackend) bool {
	return b == "none" || b == "tailcat" || b == "external"
}

func validTraceContentKind(k engine.DiagnosticContentKind) bool {
	switch k {
	case "text", "image", "files", "mixed", "unknown":
		return true
	}
	return false
}

// parseTraceParent strictly parses W3C Trace Context v00 traceparent values.
func parseTraceParent(s string) (trace.SpanContext, bool) {
	if len(s) != 55 || s[0] != '0' || s[1] != '0' || s[2] != '-' || s[35] != '-' || s[52] != '-' {
		return trace.SpanContext{}, false
	}
	for _, part := range []string{s[3:35], s[36:52], s[53:55]} {
		for i := 0; i < len(part); i++ {
			c := part[i]
			if (c < '0' || c > '9') && (c < 'a' || c > 'f') {
				return trace.SpanContext{}, false
			}
		}
	}
	var tid trace.TraceID
	if _, err := hex.Decode(tid[:], []byte(s[3:35])); err != nil || !tid.IsValid() {
		return trace.SpanContext{}, false
	}
	var sid trace.SpanID
	if _, err := hex.Decode(sid[:], []byte(s[36:52])); err != nil || !sid.IsValid() {
		return trace.SpanContext{}, false
	}
	var fb [1]byte
	if _, err := hex.Decode(fb[:], []byte(s[53:55])); err != nil {
		return trace.SpanContext{}, false
	}
	flags := trace.TraceFlags(fb[0] & byte(trace.FlagsSampled))
	sc := trace.NewSpanContext(trace.SpanContextConfig{
		TraceID:    tid,
		SpanID:     sid,
		TraceFlags: flags,
		Remote:     true,
	})
	return sc, sc.IsValid()
}

func (g *generation) OnStart(context.Context, sdktrace.ReadWriteSpan) {}
func (g *generation) OnEnd(span sdktrace.ReadOnlySpan) {
	g.mu.Lock()
	defer g.mu.Unlock()
	if !g.active || !span.SpanContext().IsSampled() {
		return
	}
	select {
	case g.spans <- span:
	default:
		g.c.drop(4, 0, 1)
	}
}
func (g *generation) runTraces() {
	defer func() { g.cleanup(); close(g.done) }()
	period := traceExportPeriod
	if g.c.opts.TraceInterval > 0 {
		period = g.c.opts.TraceInterval
	}
	timer := time.NewTicker(period)
	defer timer.Stop()
	batch := make([]sdktrace.ReadOnlySpan, 0, traceBatchCapacity)
	export := func(ctx context.Context) {
		if len(batch) == 0 {
			return
		}
		if g.ctx.Err() != nil {
			g.c.drop(4, 4, uint64(len(batch)))
			clear(batch)
			batch = batch[:0]
			return
		}
		err := g.traceExporter.ExportSpans(ctx, batch)
		if err != nil {
			reason := 3
			if g.ctx.Err() != nil {
				reason = 4
			}
			g.c.drop(4, reason, uint64(len(batch)))
		}
		g.c.exported(4, err, g.ctx, int(g.wire.status.Load()))
		clear(batch)
		batch = batch[:0]
	}
	drain := func(ctx context.Context) {
		// Admission is bounded, but concurrent producers must not extend a flush
		// indefinitely. Drain only the queue observed when the flush starts.
		n := len(g.spans)
		for i := 0; i < n; i++ {
			batch = append(batch, <-g.spans)
			if len(batch) == traceBatchCapacity {
				export(ctx)
			}
		}
	}
	for {
		select {
		case <-g.ctx.Done():
			if n := len(batch) + len(g.spans); n > 0 {
				g.c.drop(4, 4, uint64(n))
			}
			clear(batch)
			for len(g.spans) > 0 {
				<-g.spans
			}
			return
		case span := <-g.spans:
			batch = append(batch, span)
			if len(batch) == traceBatchCapacity {
				export(g.ctx)
			}
		case <-timer.C:
			drain(g.ctx)
			export(g.ctx)
		case req := <-g.flush:
			drain(req.ctx)
			export(req.ctx)
			close(req.done)
		}
	}
}
