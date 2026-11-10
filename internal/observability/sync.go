package observability

import (
	"context"
	"time"

	"github.com/uniclipboard/unic/internal/syncdiag"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

// SetSyncCollector installs the built-in collector at host assembly. It never
// runs on a business callback. The generation worker owns its ten-second timer.
func (c *Controller) SetSyncCollector(f func(uint64) ([]syncdiag.Record, bool)) {
	c.mu.Lock()
	c.syncCollector = f
	c.mu.Unlock()
	c.WakeSync()
}
func (c *Controller) SyncEpoch() uint64 {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.metrics == nil || c.closed {
		return 0
	}
	return c.metrics.epoch
}
func (c *Controller) WakeSync() {
	select {
	case c.syncWake <- struct{}{}:
	default:
	}
}
func (c *Controller) RecordSync(r syncdiag.Record) {
	c.mu.Lock()
	g := c.metrics
	c.mu.Unlock()
	if g == nil || r.Epoch != g.epoch {
		return
	}
	if !syncdiag.Valid(r) {
		c.drop(2, 2, 1)
		return
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	if !g.active {
		return
	}
	select {
	case g.diagnostics <- metricRecord{sync: &r}:
	default:
		c.drop(2, 0, 1)
	}
}
func (g *generation) collectSync() bool {
	g.c.mu.Lock()
	f := g.c.syncCollector
	g.c.mu.Unlock()
	if f == nil {
		return false
	}
	// A faulty observer must not terminate the exporter worker.
	records, more := safeSyncCollect(f, g.epoch)
	for _, r := range records {
		if r.Epoch == g.epoch && syncdiag.Valid(r) {
			g.recordSync(r)
		}
	}
	return more
}
func safeSyncCollect(f func(uint64) ([]syncdiag.Record, bool), epoch uint64) (records []syncdiag.Record, more bool) {
	defer func() { _ = recover() }()
	return f(epoch)
}

func (g *generation) initSyncMetrics() error {
	m := g.meter.Meter("unic.observability", metric.WithInstrumentationVersion("1"))
	g.inst.syncCounts = map[string]metric.Int64Counter{}
	g.inst.syncTimes = map[string]metric.Float64Histogram{}
	for name, unit := range map[string]string{"connection.attempts": "{attempt}", "connection.errors": "{error}", "session.ends": "{session}", "recovery.ends": "{episode}", "path.samples": "{sample}", "path.transitions": "{transition}", "transfer.bytes": "By", "remote_applications": "{application}"} {
		v, err := m.Int64Counter("unic.sync."+name, metric.WithUnit(unit))
		if err != nil {
			return err
		}
		g.inst.syncCounts[name] = v
	}
	var err error
	g.inst.exposure, err = m.Float64Counter("unic.sync.session.observed_time", metric.WithUnit("s"))
	if err != nil {
		return err
	}
	for _, name := range []string{"connection.duration", "session.duration", "recovery.duration", "recovery.pending", "path.upgrade_duration", "transfer.duration", "transfer.size"} {
		bounds := []float64{.001, .005, .01, .025, .05, .1, .25, .5, 1, 2.5, 5, 10, 30, 60, 120, 300, 900, 3600}
		unit := "s"
		if name == "transfer.size" {
			bounds = []float64{0, 1024, 65536, 1048576, 16777216, 67108864, 1073741824, 4294967296}
			unit = "By"
		}
		v, err := m.Float64Histogram("unic.sync."+name, metric.WithUnit(unit), metric.WithExplicitBucketBoundaries(bounds...))
		if err != nil {
			return err
		}
		g.inst.syncTimes[name] = v
	}
	return nil
}
func (g *generation) recordSync(r syncdiag.Record) {
	ctx := context.Background()
	a := func(k, v string) attribute.KeyValue { return attribute.String(k, v) }
	count := func(name string, n int64, attrs ...attribute.KeyValue) {
		g.inst.syncCounts[name].Add(ctx, n, metric.WithAttributes(attrs...))
	}
	hist := func(name string, d time.Duration, attrs ...attribute.KeyValue) {
		g.inst.syncTimes[name].Record(ctx, d.Seconds(), metric.WithAttributes(attrs...))
	}
	switch r.Kind {
	case "connection":
		attrs := []attribute.KeyValue{a("phase", r.Phase), a("direction", r.Direction), a("result", r.Result)}
		count("connection.attempts", 1, attrs...)
		hist("connection.duration", r.Duration, attrs...)
		if r.Result == "failed" {
			count("connection.errors", 1, a("phase", r.Phase), a("reason", r.Reason))
		}
	case "session":
		v := "false"
		if r.Expected {
			v = "true"
		}
		count("session.ends", 1, a("reason", r.Reason), a("expected", v))
		hist("session.duration", r.Duration, a("expected", v))
	case "exposure":
		g.inst.exposure.Add(ctx, r.Duration.Seconds())
	case "recovery":
		count("recovery.ends", 1, a("result", r.Result))
		if r.Result == "recovered" {
			hist("recovery.duration", r.Duration)
		}
	case "pending":
		hist("recovery.pending", r.Duration)
	case "path":
		count("path.samples", 1, a("direction", r.Direction), a("path", r.Path))
	case "transition":
		count("path.transitions", 1, a("from", r.From), a("to", r.To))
	case "upgrade":
		hist("path.upgrade_duration", r.Duration)
	case "transfer_size":
		g.inst.syncTimes["transfer.size"].Record(ctx, float64(r.Declared), metric.WithAttributes(a("direction", r.Direction), a("content_kind", r.ContentKind)))
	case "transfer":
		count("transfer.bytes", r.Bytes, a("direction", r.Direction), a("result", r.Result))
		hist("transfer.duration", r.Duration, a("direction", r.Direction), a("result", r.Result), a("start_path", r.Path), a("size_class", syncdiag.SizeClass(r.Declared)))
	}
}
