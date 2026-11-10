package observability

import (
	"context"
	"errors"
	"log/slog"
	"sync"
	"time"

	"github.com/uniclipboard/unic/engine"
)

// Options is host-only. Test hosts can shorten intervals without adding
// production TOML switches. The production zero values enforce the contract.
type Options struct {
	// LocalOutput mirrors already-sanitized JSON on the local worker only.
	LocalOutput       func([]byte)
	Dir               string
	Config            Config
	Version           string
	Changed           func()
	Token             func(context.Context) (string, error)
	MetricsInterval   time.Duration
	LogInterval       time.Duration
	AnalyticsInterval time.Duration
}
type Controller struct {
	failures       [4][7]uint64
	effective      bool
	lastLocalError time.Time
	control        sync.Mutex
	mu             sync.Mutex
	cfg            Config
	endpoint       string
	opts           Options
	overrides      [2]override
	closed         bool
	local          chan []byte
	localStop      chan struct{}
	localDone      chan struct{}
	logs           *generation
	metrics        *generation
	states         [4]SignalState
	drops          [4][5]uint64
	logger         *slog.Logger
	analytics      *analyticsGeneration
	analyticsReady bool
	identityReset  bool
}

func New(o Options) *Controller {
	c := &Controller{cfg: o.Config, endpoint: o.Config.Telemetry.Endpoint, opts: o, local: make(chan []byte, 1024), localStop: make(chan struct{}), localDone: make(chan struct{})}
	for i := range c.states {
		c.states[i].State = "disabled"
	}
	c.logger = slog.New(&handler{c: c})
	go c.localWorker()
	c.Apply(o.Config)
	return c
}
func (c *Controller) Logger() *slog.Logger { return c.logger }
func (c *Controller) changed() {
	if c.opts.Changed != nil {
		c.opts.Changed()
	}
}
func (c *Controller) expireLocked(i int) bool {
	o := &c.overrides[i]
	if o.level != "" && !time.Now().Before(o.deadline) {
		o.level = ""
		if o.timer != nil {
			o.timer.Stop()
			o.timer = nil
		}
		return true
	}
	return false
}
func (c *Controller) levelLocked(i int) LevelState {
	configured := c.cfg.Logging.Level
	if i == 1 {
		configured = c.cfg.Telemetry.Logs.Level
	}
	st := LevelState{Configured: configured, Effective: configured, Source: "config", Dropped: c.states[i].Dropped}
	if o := c.overrides[i]; o.level != "" {
		st.Effective = o.level
		st.Source = "override"
		ms := o.deadline.UnixMilli()
		st.Expires = &ms
	}
	return st
}
func (c *Controller) expireLevelsLocked() bool {
	a := c.expireLocked(0)
	b := c.expireLocked(1)
	return a || b
}
func (c *Controller) Levels() LogLevelState {
	c.mu.Lock()
	expired := c.expireLevelsLocked()
	state := LogLevelState{Local: c.levelLocked(0), Remote: c.levelLocked(1), Enabled: c.effective && !c.closed, RemoteEnabled: c.logs != nil && !c.closed}
	c.mu.Unlock()
	if expired {
		c.changed()
	}
	return state
}

func (c *Controller) SetLevel(target, level string, seconds int, reset bool) (LogLevelState, error) {
	i := 0
	if target == "remote" {
		i = 1
	} else if target != "local" {
		return LogLevelState{}, errors.New("invalid target")
	}
	if !reset {
		if _, ok := Level(level); !ok || seconds < 1 || seconds > 3600 {
			return LogLevelState{}, errors.New("invalid level or duration")
		}
	}
	c.control.Lock()
	defer c.control.Unlock()
	c.mu.Lock()
	if c.closed {
		c.mu.Unlock()
		return LogLevelState{}, errors.New("unavailable")
	}
	o := &c.overrides[i]
	if o.timer != nil {
		o.timer.Stop()
	}
	*o = override{}
	if !reset {
		deadline := time.Now().Add(time.Duration(seconds) * time.Second)
		o.level = level
		o.deadline = deadline
		o.timer = time.AfterFunc(time.Until(deadline), func() {
			c.mu.Lock()
			expired := c.overrides[i].deadline.Equal(deadline) && c.expireLocked(i)
			c.mu.Unlock()
			if expired {
				c.changed()
			}
		})
	}
	c.mu.Unlock()
	c.changed()
	c.logger.Info("diagnostics.changed", "target", target, "source", "override")
	return c.Levels(), nil
}
func (c *Controller) Telemetry() TelemetryState {
	c.mu.Lock()
	defer c.mu.Unlock()
	restart := []string{}
	if c.cfg.Telemetry.Endpoint != c.endpoint {
		restart = append(restart, "telemetry.endpoint")
	}
	pending := int64(0)
	if c.analytics != nil {
		pending = c.analytics.pending.Load()
	}
	return TelemetryState{Configured: c.cfg.Telemetry.Enabled, Effective: c.effective && !c.closed, Restart: restart, Credential: c.states[1].Requested && c.logs != nil && c.logs.credential || c.states[2].Requested && c.metrics != nil && c.metrics.credential || c.analytics != nil, Logs: copySignalState(c.states[1]), Metrics: copySignalState(c.states[2]), Analytics: AnalyticsState{SignalState: copySignalState(c.states[3]), Pending: pending}}
}

// Apply serializes reconfiguration. Detaching a generation closes admission;
// waiting for its cancelled worker supplies the no-more-requests barrier.
func (c *Controller) Apply(cfg Config) {
	c.control.Lock()
	defer c.control.Unlock()
	c.mu.Lock()
	if c.closed {
		c.mu.Unlock()
		return
	}
	c.cfg = cfg
	c.states[1].Requested = cfg.Telemetry.Enabled && cfg.Telemetry.Logs.Enabled
	c.states[2].Requested = cfg.Telemetry.Enabled && cfg.Telemetry.Metrics.Enabled
	c.mu.Unlock()
	for _, i := range []int{1, 2} {
		want := cfg.Telemetry.Enabled
		if i == 1 {
			want = want && cfg.Telemetry.Logs.Enabled
		} else {
			want = want && cfg.Telemetry.Metrics.Enabled
		}
		c.mu.Lock()
		g := c.logs
		if i == 2 {
			g = c.metrics
		}
		c.states[i].Requested = want
		if !want && g != nil {
			if i == 1 {
				c.logs = nil
			} else {
				c.metrics = nil
			}
			c.states[i].State = "stopping"
		}
		c.mu.Unlock()
		if !want {
			if g != nil {
				g.stop(false, context.Background())
			}
			c.mu.Lock()
			c.states[i].State = "disabled"
			c.states[i].Error = nil
			c.mu.Unlock()
			continue
		}
		if g != nil {
			continue
		}
		c.mu.Lock()
		c.states[i].State = "starting"
		c.mu.Unlock()
		var err error
		if c.endpoint == "" {
			err = errors.New("not_configured")
		} else {
			g, err = c.newGeneration(i)
		}
		c.mu.Lock()
		if err != nil {
			code := "initialization_failed"
			if err.Error() == "not_configured" {
				code = "not_configured"
			}
			if c.endpoint == "" && cfg.Telemetry.Endpoint != "" {
				code = "restart_required"
			}
			c.states[i].State = "degraded"
			c.states[i].Error = &code
		} else {
			if i == 1 {
				c.logs = g
			} else {
				c.metrics = g
			}
			c.states[i].State = "active"
			c.states[i].Error = nil
		}
		c.mu.Unlock()
	}
	c.applyAnalytics(cfg)
	c.mu.Lock()
	c.effective = cfg.Telemetry.Enabled
	c.mu.Unlock()
	c.changed()
}
func (c *Controller) drop(i, reason int, n uint64) {
	c.mu.Lock()
	c.states[i].Dropped += n
	c.drops[i][reason] += n
	c.mu.Unlock()
}
func (c *Controller) exported(i int, err error, ctx context.Context, status int) {
	if ctx.Err() != nil {
		return
	}
	c.mu.Lock()
	previous := c.states[i].State
	if err == nil {
		c.states[i].State = "active"
		c.states[i].Error = nil
	} else {
		code := exportCode(err, status)
		c.states[i].State = "degraded"
		c.states[i].Error = &code
		c.states[i].Failures++
		for j, name := range exportClasses {
			if name == code {
				c.failures[i][j]++
				break
			}
		}
	}
	changed := previous != c.states[i].State
	c.mu.Unlock()
	if changed {
		c.changed()
		if err != nil {
			c.localDegraded(exportCode(err, status))
		}
	}
}
func (c *Controller) Record(_ context.Context, r engine.DiagnosticRecord) {
	c.mu.Lock()
	g := c.metrics
	c.mu.Unlock()
	if g == nil {
		return
	}
	if !validRecord(&r) {
		c.drop(2, 2, 1)
		return
	}
	g.enqueueMetric(r)
}
func (c *Controller) Close(ctx context.Context) {
	c.control.Lock()
	defer c.control.Unlock()
	c.mu.Lock()
	if c.closed {
		c.mu.Unlock()
		return
	}
	c.closed = true
	logs, metrics, analytics := c.logs, c.metrics, c.analytics
	c.analytics = nil
	c.logs = nil
	c.metrics = nil
	for i := range c.overrides {
		if c.overrides[i].timer != nil {
			c.overrides[i].timer.Stop()
		}
	}
	c.mu.Unlock()
	// All remote signals share one budget and flush concurrently.
	closeCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	var wg sync.WaitGroup
	for _, g := range []*generation{logs, metrics} {
		if g != nil {
			wg.Add(1)
			go func() { defer wg.Done(); g.stop(true, closeCtx) }()
		}
	}
	if analytics != nil {
		wg.Add(1)
		go func() { defer wg.Done(); analytics.stop(true, closeCtx) }()
	}
	close(c.localStop)
	wg.Wait()
	select {
	case <-c.localDone:
	case <-closeCtx.Done():
	}
}

func copySignalState(s SignalState) SignalState {
	if s.Error != nil {
		code := *s.Error
		s.Error = &code
	}
	return s
}
