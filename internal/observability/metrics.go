package observability

import (
	"context"
	"strings"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/exemplar"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"

	"github.com/uniclipboard/unic/engine"
)

var exportClasses = []string{"dns", "tls", "timeout", "rate_limited", "unauthorized", "unavailable", "internal"}

var stages = map[string]map[string]bool{
	"observe":    {"accepted": true, "baseline": true, "loop_prevented": true, "policy_filtered": true, "failed": true},
	"process":    {"succeeded": true, "failed": true, "cancelled": true},
	"commit":     {"succeeded": true, "failed": true, "cancelled": true},
	"send_queue": {"started": true, "cancelled": true, "superseded": true},
	"send":       {"succeeded": true, "failed": true, "cancelled": true, "superseded": true},
	"receive":    {"succeeded": true, "duplicate": true, "policy_filtered": true, "rejected": true, "failed": true, "cancelled": true},
	"apply":      {"succeeded": true, "failed": true, "cancelled": true, "deferred": true},
}

const codes = " invalid_argument invalid_state closed in_use conflict not_found resource_expired unsupported permission_denied sync_disabled suspended no_targets limit_exceeded resource_exhausted unavailable timeout cancelled identity_mismatch protocol_mismatch source_changed incomplete invalid expired rate_limited resync_required internal "

func validCode(v string) bool { return v != "" && strings.Contains(codes, " "+v+" ") }
func validRecord(r *engine.DiagnosticRecord) bool {
	if !stages[string(r.Stage)][string(r.Result)] || r.Duration < 0 {
		return false
	}
	if r.Backend != "none" && r.Backend != "tailcat" && r.Backend != "external" {
		return false
	}
	switch r.ContentKind {
	case "text", "image", "files", "mixed", "unknown":
	default:
		return false
	}
	if r.Code != "" && !validCode(string(r.Code)) {
		r.Code = engine.CodeInternal
	}
	switch r.Result {
	case "failed", "rejected":
		if r.Code == "" {
			r.Code = engine.CodeInternal
		}
	case "cancelled":
		r.Code = engine.CodeCancelled
	case "deferred":
	default:
		if r.Code != "" {
			return false
		}
	}
	return true
}

type instruments struct {
	syncCounts   map[string]metric.Int64Counter
	syncTimes    map[string]metric.Float64Histogram
	exposure     metric.Float64Counter
	operations   metric.Int64Counter
	duration     metric.Float64Histogram
	errors       metric.Int64Counter
	dropped      metric.Int64Counter
	failures     metric.Int64Counter
	lastDrops    [4][5]uint64
	lastFailures [4][7]uint64
}

func (g *generation) initMetrics() error {
	// Health counters are process totals locally. A new remote generation only
	// exports subsequent changes, never the previous generation's backlog.
	g.c.mu.Lock()
	g.inst.lastDrops = g.c.drops
	g.inst.lastFailures = g.c.failures
	g.c.mu.Unlock()
	g.reader = sdkmetric.NewManualReader(sdkmetric.WithTemporalitySelector(func(sdkmetric.InstrumentKind) metricdata.Temporality { return g.temporality() }))
	g.meter = sdkmetric.NewMeterProvider(sdkmetric.WithResource(g.resource), sdkmetric.WithReader(g.reader), sdkmetric.WithCardinalityLimit(1024), sdkmetric.WithExemplarFilter(exemplar.AlwaysOffFilter))
	m := g.meter.Meter("unic.observability", metric.WithInstrumentationVersion("1"))
	var err error
	g.inst.operations, err = m.Int64Counter("unic.pipeline.operations", metric.WithUnit("{operation}"))
	if err != nil {
		return err
	}
	g.inst.duration, err = m.Float64Histogram("unic.pipeline.duration", metric.WithUnit("s"), metric.WithExplicitBucketBoundaries(.001, .005, .01, .025, .05, .1, .25, .5, 1, 2.5, 5, 10, 30, 60))
	if err != nil {
		return err
	}
	g.inst.errors, err = m.Int64Counter("unic.pipeline.errors", metric.WithUnit("{error}"))
	if err != nil {
		return err
	}
	g.inst.dropped, err = m.Int64Counter("unic.observability.dropped", metric.WithUnit("{record}"))
	if err != nil {
		return err
	}
	g.inst.failures, err = m.Int64Counter("unic.observability.export_failures", metric.WithUnit("{export}"))
	if err != nil {
		return err
	}
	return g.initSyncMetrics()
}
func (g *generation) recordMetric(r engine.DiagnosticRecord) {
	ctx := context.Background()
	stage := attribute.String("stage", string(r.Stage))
	result := attribute.String("result", string(r.Result))
	backend := attribute.String("backend", string(r.Backend))
	g.inst.operations.Add(ctx, 1, metric.WithAttributes(stage, result, backend, attribute.String("content_kind", string(r.ContentKind))))
	if r.Stage != "observe" {
		g.inst.duration.Record(ctx, r.Duration.Seconds(), metric.WithAttributes(stage, result, backend))
	}
	if r.Result == "failed" || r.Result == "rejected" {
		g.inst.errors.Add(ctx, 1, metric.WithAttributes(stage, backend, attribute.String("code", string(r.Code))))
	}
}
func (g *generation) collectHealth() {
	g.c.mu.Lock()
	drops := g.c.drops
	failures := g.c.failures
	g.c.mu.Unlock()
	signals := []string{"local_logs", "remote_logs", "metrics", "analytics"}
	reasons := []string{"queue_full", "oversize", "invalid_record", "sink_failed", "disabled_discard"}
	for i := range signals {
		for j := range reasons {
			n := drops[i][j] - g.inst.lastDrops[i][j]
			if n > 0 {
				g.inst.dropped.Add(context.Background(), int64(n), metric.WithAttributes(attribute.String("signal", signals[i]), attribute.String("reason", reasons[j])))
			}
			g.inst.lastDrops[i][j] = drops[i][j]
		}
		for j, code := range exportClasses {
			n := failures[i][j] - g.inst.lastFailures[i][j]
			if n > 0 {
				g.inst.failures.Add(context.Background(), int64(n), metric.WithAttributes(attribute.String("signal", signals[i]), attribute.String("code", code)))
			}
			g.inst.lastFailures[i][j] = failures[i][j]
		}

	}
}

func (g *generation) temporality() metricdata.Temporality {
	if postHogEndpoint(g.c.endpoint) {
		return metricdata.DeltaTemporality
	}
	return metricdata.CumulativeTemporality
}
