package observability

import (
	"context"
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
	limits.AttributeCountLimit = 2
	limits.AttributeValueLengthLimit = 64
	// No caller receives an SDK span. Only the fixed completion attributes below
	// can be added. Parent context is carried through a private host-only key.
	g.traceProvider = sdktrace.NewTracerProvider(sdktrace.WithResource(g.resource), sdktrace.WithSampler(sdktrace.ParentBased(sdktrace.TraceIDRatioBased(ratio))), sdktrace.WithRawSpanLimits(limits), sdktrace.WithSpanProcessor(g))
	return nil
}

// StartSubmission is called only after unique host submission admission. It
// intentionally ignores caller OTel context, baggage and socket metadata.
func (c *Controller) StartSubmission(ctx context.Context) (context.Context, func(engine.DiagnosticResult, engine.Code)) {
	c.mu.Lock()
	g := c.traces
	c.mu.Unlock()
	return startTrace(ctx, g, trace.SpanContext{}, "unic.submission")
}
func (c *Controller) StartCommit(ctx context.Context) (context.Context, func(engine.DiagnosticResult, engine.Code)) {
	parent, ok := ctx.Value(traceContextKey{}).(localTrace)
	if !ok {
		return ctx, func(engine.DiagnosticResult, engine.Code) {}
	}
	return startTrace(ctx, parent.generation, parent.parent, "unic.commit")
}
func startTrace(ctx context.Context, g *generation, parent trace.SpanContext, name string) (context.Context, func(engine.DiagnosticResult, engine.Code)) {
	noop := func(engine.DiagnosticResult, engine.Code) {}
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
		clean = trace.ContextWithSpanContext(clean, parent)
	}
	_, span := g.traceProvider.Tracer("unic.observability", trace.WithInstrumentationVersion("1")).Start(clean, name)
	g.mu.Unlock()
	next := context.WithValue(ctx, traceContextKey{}, localTrace{g, span.SpanContext()})
	var once sync.Once
	return next, func(result engine.DiagnosticResult, code engine.Code) {
		once.Do(func() {
			switch result {
			case "succeeded":
				code = ""
			case "cancelled":
				code = engine.CodeCancelled
			case "failed":
				if !validCode(string(code)) {
					code = engine.CodeInternal
				}
			default:
				result = "failed"
				code = engine.CodeInternal
			}
			span.SetAttributes(attribute.String("result", string(result)))
			if code != "" {
				span.SetAttributes(attribute.String("code", string(code)))
				span.SetStatus(otelcodes.Error, "")
			}
			span.End()
		})
	}
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
	timer := time.NewTicker(traceExportPeriod)
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
