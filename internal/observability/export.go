package observability

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"log/slog"
	"net"
	"net/http"
	"runtime"
	"strings"
	"sync"
	"time"

	"go.opentelemetry.io/contrib/bridges/otelslog"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/exporters/otlp/otlplog/otlploghttp"
	"go.opentelemetry.io/otel/exporters/otlp/otlpmetric/otlpmetrichttp"
	sdklog "go.opentelemetry.io/otel/sdk/log"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
	"go.opentelemetry.io/otel/sdk/resource"
	common "go.opentelemetry.io/proto/otlp/common/v1"
	resourcepb "go.opentelemetry.io/proto/otlp/resource/v1"

	"github.com/uniclipboard/unic/engine"
)

type flushRequest struct {
	ctx  context.Context
	done chan struct{}
}
type generation struct {
	wire           *privateTransport
	c              *Controller
	index          int
	ctx            context.Context
	cancel         context.CancelFunc
	mu             sync.Mutex
	active         bool
	done           chan struct{}
	flush          chan flushRequest
	records        chan sdklog.Record
	diagnostics    chan engine.DiagnosticRecord
	handler        slog.Handler
	provider       *sdklog.LoggerProvider
	logExporter    *otlploghttp.Exporter
	meter          *sdkmetric.MeterProvider
	reader         *sdkmetric.ManualReader
	metricExporter *otlpmetrichttp.Exporter
	resource       *resource.Resource
	inst           instruments
	credential     bool
	transport      *http.Transport
}

func (c *Controller) newGeneration(i int) (*generation, error) {
	ctx, cancel := context.WithCancel(context.Background())
	g := &generation{c: c, index: i, ctx: ctx, cancel: cancel, active: true, done: make(chan struct{}), flush: make(chan flushRequest), records: make(chan sdklog.Record, 1024), diagnostics: make(chan engine.DiagnosticRecord, 1024)}
	version := c.opts.Version
	if version == "" {
		version = "dev"
	}
	g.resource = resource.NewSchemaless(attribute.String("service.name", "unicd"), attribute.String("service.version", version), attribute.String("os.type", runtime.GOOS), attribute.String("host.arch", runtime.GOARCH))
	// A private client overrides SDK environment TLS/proxy/timeout settings.
	// Root CAs follow the host trust store (SSL_CERT_FILE is used by E2E).
	g.transport = &http.Transport{Proxy: http.ProxyFromEnvironment, DialContext: (&net.Dialer{Timeout: 3 * time.Second, KeepAlive: 30 * time.Second}).DialContext, TLSClientConfig: &tls.Config{MinVersion: tls.VersionTLS12}, TLSHandshakeTimeout: 3 * time.Second, MaxIdleConnsPerHost: 1, MaxConnsPerHost: 1, IdleConnTimeout: 30 * time.Second}
	wireResource := &resourcepb.Resource{Attributes: []*common.KeyValue{resourceAttribute("service.name", "unicd"), resourceAttribute("service.version", version), resourceAttribute("os.type", runtime.GOOS), resourceAttribute("host.arch", runtime.GOARCH)}}
	g.wire = &privateTransport{next: g.transport, resource: wireResource}
	client := &http.Client{Transport: g.wire, Timeout: 3 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	headers := map[string]string{}
	if c.opts.Token != nil {
		readCtx, done := context.WithTimeout(ctx, 3*time.Second)
		token, err := c.opts.Token(readCtx)
		done()
		if err != nil {
			cancel()
			g.transport.CloseIdleConnections()
			return nil, errors.New("initialization_failed")
		}
		if token != "" {
			if strings.ContainsAny(token, "\r\n") {
				cancel()
				return nil, errors.New("initialization_failed")
			}
			headers["Authorization"] = "Bearer " + token
			g.credential = true
		}
	}
	endpoint := strings.TrimSuffix(c.endpoint, "/")
	var err error
	if i == 1 {
		g.logExporter, err = otlploghttp.New(ctx, otlploghttp.WithEndpointURL(endpoint+"/v1/logs"), otlploghttp.WithHTTPClient(client), otlploghttp.WithTLSClientConfig(g.transport.TLSClientConfig), otlploghttp.WithHeaders(headers), otlploghttp.WithTimeout(3*time.Second), otlploghttp.WithRetry(otlploghttp.RetryConfig{Enabled: false}), otlploghttp.WithCompression(otlploghttp.NoCompression))
		if err == nil {
			g.provider = sdklog.NewLoggerProvider(sdklog.WithResource(g.resource), sdklog.WithProcessor(g), sdklog.WithAttributeCountLimit(7), sdklog.WithAttributeValueLengthLimit(256), sdklog.WithAttributeValueDepthLimit(1))
			g.handler = otelslog.NewHandler("unic.observability", otelslog.WithVersion("1"), otelslog.WithLoggerProvider(g.provider), otelslog.WithSource(false))
		}
	} else {
		g.metricExporter, err = otlpmetrichttp.New(ctx, otlpmetrichttp.WithEndpointURL(endpoint+"/v1/metrics"), otlpmetrichttp.WithHTTPClient(client), otlpmetrichttp.WithTLSClientConfig(g.transport.TLSClientConfig), otlpmetrichttp.WithHeaders(headers), otlpmetrichttp.WithTimeout(3*time.Second), otlpmetrichttp.WithRetry(otlpmetrichttp.RetryConfig{Enabled: false}), otlpmetrichttp.WithCompression(otlpmetrichttp.NoCompression), otlpmetrichttp.WithTemporalitySelector(func(sdkmetric.InstrumentKind) metricdata.Temporality { return metricdata.CumulativeTemporality }))
		if err == nil {
			err = g.initMetrics()
		}
	}
	if err != nil {
		cancel()
		g.cleanup()
		return nil, errors.New("initialization_failed")
	}
	go g.run()
	return g, nil
}

// This is the SDK's public Processor seam. It supplies the only remote-log
// queue, so queue rejection is measurable and disable never triggers a flush.
func (g *generation) Enabled(context.Context, sdklog.EnabledParameters) bool {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.active
}
func (g *generation) OnEmit(_ context.Context, r *sdklog.Record) error {
	g.mu.Lock()
	defer g.mu.Unlock()
	if !g.active {
		return nil
	}
	select {
	case g.records <- r.Clone():
	default:
		g.c.drop(1, 0, 1)
	}
	return nil
}
func (g *generation) ForceFlush(ctx context.Context) error { return g.forceFlush(ctx) }
func (g *generation) Shutdown(ctx context.Context) error   { return g.forceFlush(ctx) }
func (g *generation) enqueueMetric(r engine.DiagnosticRecord) {
	g.mu.Lock()
	defer g.mu.Unlock()
	if !g.active {
		return
	}
	select {
	case g.diagnostics <- r:
	default:
		g.c.drop(2, 0, 1)
	}
}
func (g *generation) forceFlush(ctx context.Context) error {
	req := flushRequest{ctx, make(chan struct{})}
	select {
	case <-g.ctx.Done():
		return nil
	case <-g.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	case g.flush <- req:
	}
	select {
	case <-req.done:
		return nil
	case <-g.ctx.Done():
		return nil
	case <-g.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (g *generation) stop(flush bool, ctx context.Context) {
	g.mu.Lock()
	g.active = false
	g.mu.Unlock()
	if flush {
		_ = g.forceFlush(ctx)
	}
	g.cancel()
	<-g.done
}
func (g *generation) run() {
	// Cancelled generations make Processor.Shutdown a no-op, then publish done
	// after releasing every SDK and transport resource.
	defer func() { g.cleanup(); close(g.done) }()
	interval := 5 * time.Second
	if g.index == 2 {
		interval = 60 * time.Second
		if g.c.opts.MetricsInterval > 0 {
			interval = g.c.opts.MetricsInterval
		}
	} else if g.c.opts.LogInterval > 0 {
		interval = g.c.opts.LogInterval
	}
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	batch := make([]sdklog.Record, 0, 128)
	export := func(ctx context.Context) {
		if ctx.Err() != nil {
			return
		}
		if g.index == 1 {
			if len(batch) == 0 {
				return
			}
			n := len(batch)
			err := g.logExporter.Export(ctx, batch)
			if err != nil {
				reason := 3
				if g.ctx.Err() != nil {
					reason = 4
				}
				g.c.drop(1, reason, uint64(n))
			}
			clear(batch)
			batch = batch[:0]
			g.c.exported(1, err, g.ctx, int(g.wire.status.Load()))
		} else {
			g.collectHealth()
			var data metricdata.ResourceMetrics
			err := g.reader.Collect(ctx, &data)
			if err == nil {
				data.Resource = g.resource
				err = g.metricExporter.Export(ctx, &data)
			}
			g.c.exported(2, err, g.ctx, int(g.wire.status.Load()))
		}
	}
	drain := func() {
		for {
			select {
			case r := <-g.records:
				batch = append(batch, r)
				if len(batch) == 128 {
					export(g.ctx)
				}
			case r := <-g.diagnostics:
				g.recordMetric(r)
			default:
				return
			}
		}
	}
	for {
		select {
		case <-g.ctx.Done():
			n := len(g.records) + len(batch)
			if g.index == 2 {
				n = len(g.diagnostics)
			}
			if n > 0 {
				g.c.drop(g.index, 4, uint64(n))
			}
			return
		case r := <-g.records:
			batch = append(batch, r)
			if len(batch) == 128 {
				export(g.ctx)
			}
		case r := <-g.diagnostics:
			g.recordMetric(r)
		case <-ticker.C:
			drain()
			export(g.ctx)
		case req := <-g.flush:
			drain()
			export(req.ctx)
			close(req.done)
		}
	}
}
func (g *generation) cleanup() {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if g.provider != nil {
		_ = g.provider.Shutdown(ctx)
	}
	if g.logExporter != nil {
		_ = g.logExporter.Shutdown(ctx)
	}
	if g.meter != nil {
		_ = g.meter.Shutdown(ctx)
	}
	if g.metricExporter != nil {
		_ = g.metricExporter.Shutdown(ctx)
	}
	if g.transport != nil {
		g.transport.CloseIdleConnections()
	}
}
func exportCode(err error, status int) string {
	switch status {
	case 401, 403:
		return "unauthorized"
	case 429:
		return "rate_limited"
	}
	var dns *net.DNSError
	var cert x509.UnknownAuthorityError
	var host x509.HostnameError
	var neterr net.Error
	switch {
	case errors.As(err, &dns):
		return "dns"
	case errors.As(err, &cert), errors.As(err, &host):
		return "tls"
	case errors.Is(err, context.DeadlineExceeded):
		return "timeout"
	case errors.As(err, &neterr) && neterr.Timeout():
		return "timeout"
	}
	// SDK errors stay local. Only the following finite class escapes.
	s := err.Error()
	switch {
	case strings.Contains(s, "429"):
		return "rate_limited"
	case strings.Contains(s, "401"), strings.Contains(s, "403"):
		return "unauthorized"
	case strings.Contains(s, "tls"), strings.Contains(s, "certificate"):
		return "tls"
	}
	return "unavailable"
}
