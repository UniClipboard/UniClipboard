package observability

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net/http"
	"os"
	"regexp"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/posthog/posthog-go"
	"github.com/uniclipboard/unic/engine"
)

var releaseVersionPattern = regexp.MustCompile(`^[A-Za-z0-9._+-]{1,64}$`)

func safeVersion(v string) string {
	if !releaseVersionPattern.MatchString(v) {
		return "dev"
	}
	return v
}

// analyticsGeneration retains no business object or caller context. A single
// admission counter bounds all SDK queues, its batch and in-flight events.
type analyticsGeneration struct {
	c         *Controller
	client    posthog.Client
	ctx       context.Context
	cancel    context.CancelFunc
	mu        sync.Mutex
	active    bool
	pending   atomic.Int64
	identity  string
	transport *http.Transport
	host      string
}

func (c *Controller) newAnalytics() (*analyticsGeneration, error) {
	if c.endpoint == "" || c.opts.Token == nil {
		return nil, errors.New("not_configured")
	}
	ctx, cancel := context.WithCancel(context.Background())
	tokenCtx, done := context.WithTimeout(ctx, 3*time.Second)
	token, err := c.opts.Token(tokenCtx)
	done()
	if err != nil || !strings.HasPrefix(token, "phc_") || strings.ContainsAny(token, "\r\n\t ") {
		cancel()
		return nil, errors.New("not_configured")
	}
	id, err := analyticsIdentity(c.opts.Dir)
	if err != nil {
		cancel()
		return nil, errors.New("identity_unavailable")
	}
	// APIfy lazily probes OS information in this SDK release. Warm its cache
	// on the lifecycle/control path so the first business Enqueue never does I/O.
	_ = (posthog.Capture{}).APIfy()
	g := &analyticsGeneration{c: c, ctx: ctx, cancel: cancel, identity: id, host: analyticsHost(c.endpoint), active: true, transport: &http.Transport{Proxy: http.ProxyFromEnvironment, MaxConnsPerHost: 1, MaxIdleConnsPerHost: 1, IdleConnTimeout: 30 * time.Second, TLSHandshakeTimeout: 3 * time.Second}}
	interval := 5 * time.Second
	if c.opts.AnalyticsInterval > 0 {
		interval = c.opts.AnalyticsInterval
	}
	client, err := posthog.NewWithConfig(token, posthog.Config{
		Endpoint: g.host, Interval: interval, MaxQueueSize: 896, MaxEnqueuedRequests: 1, BatchSize: 64,
		BatchSubmitTimeout: -time.Millisecond, BatchUploadTimeout: 3 * time.Second, ShutdownTimeout: 2 * time.Second,
		MaxRetries: posthog.Ptr(0), FeatureFlagRequestMaxRetries: posthog.Ptr(0), DisableGeoIP: posthog.Ptr(true), IsServer: posthog.Ptr(false),
		Transport: g, Callback: g, Logger: posthog.StdLogger(log.New(io.Discard, "", 0), false),
	})
	if err != nil {
		cancel()
		g.transport.CloseIdleConnections()
		return nil, errors.New("initialization_failed")
	}
	g.client = client
	return g, nil
}
func (g *analyticsGeneration) Success(posthog.APIMessage) { g.pending.Add(-1) }
func (g *analyticsGeneration) Failure(_ posthog.APIMessage, _ error) {
	g.pending.Add(-1)
	reason := 3
	if g.ctx.Err() != nil {
		reason = 4
	}
	g.c.drop(3, reason, 1)
}
func validProductEvent(e engine.ProductEvent) bool {
	switch e.Name {
	case "app_started", "pairing_completed", "history_applied", "manual_send_completed", "action_completed":
	default:
		return false
	}
	switch e.Result {
	case "", "succeeded", "failed", "cancelled", "superseded", "partial", "skipped":
	default:
		return false
	}
	switch e.Product {
	case "", "cli", "desktop":
	default:
		return false
	}
	switch e.ContentKind {
	case "", "text", "image", "files", "mixed", "unknown":
	default:
		return false
	}
	switch e.ActionCategory {
	case "", "builtin", "external":
	default:
		return false
	}
	return e.Code == "" || validCode(string(e.Code))
}

// ProductEvent only validates and enqueues bounded values. No file/network I/O.
func (c *Controller) ProductEvent(_ context.Context, e engine.ProductEvent) {
	c.mu.Lock()
	g := c.analytics
	c.mu.Unlock()
	if g == nil {
		return
	}
	if !validProductEvent(e) {
		c.drop(3, 2, 1)
		return
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	if !g.active {
		return
	}
	for {
		n := g.pending.Load()
		if n >= 1024 {
			c.drop(3, 0, 1)
			return
		}
		if g.pending.CompareAndSwap(n, n+1) {
			break
		}
	}
	product := e.Product
	if product == "" {
		product = "cli"
	}
	props := posthog.Properties{"schema_version": 1, "product": product, "product_version": safeVersion(c.opts.Version), "os": runtime.GOOS, "arch": runtime.GOARCH, "$process_person_profile": false, "$geoip_disable": true}
	if e.Result != "" {
		props["result"] = e.Result
	}
	if e.Code != "" {
		props["code"] = string(e.Code)
	}
	if e.ContentKind != "" {
		props["content_kind"] = string(e.ContentKind)
	}
	if e.ActionCategory != "" {
		props["action_category"] = e.ActionCategory
	}
	if err := g.client.Enqueue(posthog.Capture{DistinctId: g.identity, Event: e.Name, Properties: props}); err != nil {
		g.pending.Add(-1)
		c.drop(3, 0, 1)
	}
}

// The SDK adds system fields after BeforeSend. Filter the serialized batch at
// the final transport boundary and refuse redirects or ancillary SDK requests.
func (g *analyticsGeneration) RoundTrip(req *http.Request) (*http.Response, error) {
	if req.URL.Scheme+"://"+req.URL.Host != g.host || req.URL.Path != "/batch/" || req.URL.RawQuery != "" {
		return nil, errors.New("invalid analytics destination")
	}
	ctx, cancel := context.WithTimeout(req.Context(), 3*time.Second)
	stop := context.AfterFunc(g.ctx, cancel)
	release := func() { stop(); cancel() }
	returned := false
	defer func() {
		if !returned {
			release()
		}
	}()
	if g.ctx.Err() != nil {
		return nil, context.Canceled
	}
	b, err := io.ReadAll(io.LimitReader(req.Body, 512<<10))
	_ = req.Body.Close()
	if err != nil {
		return nil, errors.New("invalid analytics batch")
	}
	var batch struct {
		APIKey string                 `json:"api_key"`
		Events []posthog.CaptureInApi `json:"batch"`
	}
	if json.Unmarshal(b, &batch) != nil || len(batch.Events) > 64 {
		return nil, errors.New("invalid analytics batch")
	}
	for i := range batch.Events {
		ev := &batch.Events[i]
		p := posthog.Properties{}
		for _, key := range []string{"schema_version", "product", "product_version", "os", "arch", "result", "code", "content_kind", "backend", "action_category"} {
			if v, ok := ev.Properties[key]; ok {
				p[key] = v
			}
		}
		p["$process_person_profile"] = false
		p["$geoip_disable"] = true
		ev.Properties = p
		encoded, _ := json.Marshal(ev)
		if len(encoded) > 2048 {
			return nil, errors.New("oversize analytics event")
		}
	}
	b, err = json.Marshal(batch)
	if err != nil {
		return nil, errors.New("invalid analytics batch")
	}
	clone := req.Clone(ctx)
	clone.Body = io.NopCloser(bytes.NewReader(b))
	clone.ContentLength = int64(len(b))
	clone.GetBody = nil
	response, err := g.transport.RoundTrip(clone)
	status := 0
	if response != nil {
		status = response.StatusCode
	}
	result := err
	if result == nil && status >= 300 {
		result = errors.New("analytics export failed")
	}
	g.c.exported(3, result, g.ctx, status)
	if response != nil && response.Body != nil {
		original := response.Body
		response.Body = &analyticsResponse{Reader: io.LimitReader(original, 4096), close: func() error { defer release(); return original.Close() }}
		returned = true
	}
	return response, err
}
func (g *analyticsGeneration) stop(flush bool, ctx context.Context) {
	g.mu.Lock()
	g.active = false
	g.mu.Unlock()
	if !flush {
		g.cancel()
	}
	// A cancelled close never sends an old queue: transport cancellation precedes
	// the SDK's draining Close. Normal exit has the shared controller deadline.
	_ = g.client.CloseWithContext(ctx)
	g.cancel()
	g.transport.CloseIdleConnections()
}

// Identity operations run only under the single-owner host lifecycle, never on
// a business callback. os.Root prevents links from escaping the profile.
func analyticsIdentity(dir string) (string, error) {
	root, err := os.OpenRoot(dir)
	if err != nil {
		return "", err
	}
	defer root.Close()
	info, err := root.Lstat("analytics-id")
	if err == nil {
		if !info.Mode().IsRegular() {
			return "", errors.New("invalid identity file")
		}
		f, e := root.Open("analytics-id")
		if e != nil {
			return "", e
		}
		defer f.Close()
		actual, e := f.Stat()
		if e != nil || !os.SameFile(info, actual) {
			return "", errors.New("invalid identity file")
		}
		b, e := io.ReadAll(io.LimitReader(f, 65))
		if e != nil {
			return "", e
		}
		id := string(b)
		if id == "disabled" {
			_ = f.Close()
			if e := root.Remove("analytics-id"); e != nil {
				return "", e
			}
			return analyticsIdentity(dir)
		}
		raw, e := hex.DecodeString(id)
		if e != nil || len(raw) != 16 {
			return "", errors.New("invalid identity")
		}
		return id, nil
	}
	if !os.IsNotExist(err) {
		return "", err
	}
	var data [16]byte
	if _, err = rand.Read(data[:]); err != nil {
		return "", err
	}
	id := hex.EncodeToString(data[:])
	temp := ".analytics-" + id
	f, err := root.OpenFile(temp, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return "", err
	}
	defer root.Remove(temp)
	_, err = f.WriteString(id)
	if err == nil {
		err = f.Sync()
	}
	cerr := f.Close()
	if err == nil {
		err = cerr
	}
	if err == nil {
		err = root.Rename(temp, "analytics-id")
	}
	return id, err
}
func clearAnalyticsIdentity(dir string) error {
	root, err := os.OpenRoot(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}
	defer root.Close()
	info, err := root.Lstat("analytics-id")
	if os.IsNotExist(err) {
		return nil
	}
	if err != nil {
		return err
	}
	if info.Mode().IsRegular() {
		// Invalidate before unlinking. If unlink fails, a subsequent process
		// must never reuse the old identity after the user disabled analytics.
		f, e := root.OpenFile("analytics-id", os.O_WRONLY, 0)
		if e != nil {
			return e
		}
		actual, e := f.Stat()
		if e != nil || !os.SameFile(info, actual) {
			_ = f.Close()
			return errors.New("identity changed")
		}
		if e = f.Truncate(0); e == nil {
			_, e = f.WriteString("disabled")
		}
		if e == nil {
			e = f.Sync()
		}
		cerr := f.Close()
		if e == nil {
			e = cerr
		}
		if e != nil {
			return e
		}
	}
	err = root.Remove("analytics-id")
	if os.IsNotExist(err) {
		return nil
	}
	return err
}

// StartProduct is called after the Engine acquires its exclusive profile lock.
func (c *Controller) StartProduct(product string) {
	c.control.Lock()
	c.mu.Lock()
	first := !c.analyticsReady
	c.analyticsReady = true
	cfg := c.cfg
	c.mu.Unlock()
	if first {
		c.applyAnalytics(cfg)
	}
	c.control.Unlock()
	if first {
		c.ProductEvent(context.Background(), engine.ProductEvent{Name: "app_started", Product: product})
	}
}
func (c *Controller) applyAnalytics(cfg Config) {
	want := cfg.Telemetry.Enabled && cfg.Telemetry.Analytics.Enabled
	c.mu.Lock()
	c.states[3].Requested = want
	ready := c.analyticsReady
	g := c.analytics
	if !want {
		c.analytics = nil
		c.states[3].State = "stopping"
	}
	c.mu.Unlock()
	if !ready {
		c.mu.Lock()
		if want {
			c.states[3].State = "starting"
		} else {
			c.states[3].State = "disabled"
		}
		c.mu.Unlock()
		return
	}
	if !want {
		if g != nil {
			g.stop(false, context.Background())
		}
		err := clearAnalyticsIdentity(c.opts.Dir)
		c.mu.Lock()
		c.identityReset = err != nil
		c.states[3].State = "disabled"
		c.states[3].Error = nil
		if err != nil {
			code := "identity_unavailable"
			c.states[3].State = "degraded"
			c.states[3].Error = &code
		}
		c.mu.Unlock()
		return
	}
	if g != nil {
		return
	}
	c.mu.Lock()
	reset := c.identityReset
	c.mu.Unlock()
	var err error
	if reset {
		err = clearAnalyticsIdentity(c.opts.Dir)
	}
	if err == nil {
		g, err = c.newAnalytics()
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if err != nil {
		code := err.Error()
		if code != "not_configured" && code != "identity_unavailable" {
			code = "initialization_failed"
		}
		c.states[3].State = "degraded"
		c.states[3].Error = &code
		return
	}
	c.identityReset = false
	c.analytics = g
	c.states[3].State = "active"
	c.states[3].Error = nil
}

type analyticsResponse struct {
	io.Reader
	close func() error
}

func (b *analyticsResponse) Close() error { return b.close() }
