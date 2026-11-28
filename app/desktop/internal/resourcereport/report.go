// Package resourcereport periodically reports Desktop role process CPU, memory,
// and wakeup/context-switch deltas to the profile's single unicd OTLP exporter.
package resourcereport

import (
	"context"
	"sync"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/procmetrics"
)

const (
	defaultInterval = 60 * time.Second
	sendTimeout     = time.Second
	minExitWindow   = 10.0
	sendQueueCap    = 16
	stateQueueCap   = 16
	minSpillLimit   = 25 * time.Millisecond
	maxSpillLimit   = 250 * time.Millisecond
)

type sendItem struct {
	epoch  uint64
	ctx    context.Context
	sample observability.ResourceSample
}

type stateTransition struct {
	prev string
	next string
	at   time.Time
}

// Options configures a Desktop role resource reporter.
type Options struct {
	Role     string
	Socket   string
	Settings *config.Manager
	Interval time.Duration
	Initial  string           // "foreground" or "background"; defaults to "background"
	Now      func() time.Time // optional monotonic clock source for testing
}

// Reporter samples process resources and submits them to POST /v1/diagnostics/resource-sample
// only while telemetry.enabled && telemetry.metrics.enabled is true.
// Visibility transitions capture event timestamps non-blockingly under a tiny memory lock without
// executing OS probes or IPC. Ambiguous multi-switch, queue-overflow, or excess-spill cumulative
// windows are silently discarded while re-anchoring baseline (Decision C1).
type Reporter struct {
	role     string
	client   *api.Client
	interval time.Duration
	rootCtx  context.Context
	nowFn    func() time.Time

	notify     chan struct{}
	sendCh     chan sendItem
	senderStop chan struct{}
	senderDone chan struct{}

	// Lock ordering: mu -> samplerMu -> stateMu.
	// UI callbacks only ever acquire stateMu (never samplerMu or mu) so OS probe I/O
	// and IPC never execute or block on UI callbacks.
	stateMu       sync.Mutex
	uiState       string
	uiClosed      bool
	stateEvents   []stateTransition
	stateOverflow bool

	samplerMu sync.Mutex
	sampler   *procmetrics.Sampler

	mu         sync.Mutex
	enabled    bool
	epoch      uint64
	appState   string
	baseTime   time.Time
	stopped    bool
	sendCtx    context.Context
	sendCancel context.CancelFunc
	cancel     context.CancelFunc
	done       chan struct{}
}

// New creates and starts a Reporter bound to ctx. Call Close on role shutdown.
func New(ctx context.Context, opts Options) *Reporter {
	interval := opts.Interval
	if interval <= 0 {
		interval = defaultInterval
	}
	initialState := opts.Initial
	if initialState != "foreground" {
		initialState = "background"
	}
	nowFn := opts.Now
	if nowFn == nil {
		nowFn = time.Now
	}
	enabled := false
	if opts.Settings != nil {
		cfg := opts.Settings.Current()
		enabled = cfg.Telemetry.Enabled && cfg.Telemetry.Metrics.Enabled
	}
	rctx, cancel := context.WithCancel(ctx)
	now := nowFn()
	r := &Reporter{
		role:       opts.Role,
		client:     api.NewClient(opts.Socket),
		interval:   interval,
		rootCtx:    rctx,
		nowFn:      nowFn,
		notify:     make(chan struct{}, 1),
		sendCh:     make(chan sendItem, sendQueueCap),
		senderStop: make(chan struct{}),
		senderDone: make(chan struct{}),
		uiState:    initialState,
		sampler:    procmetrics.NewSampler(),
		enabled:    enabled,
		appState:   initialState,
		baseTime:   now,
		cancel:     cancel,
		done:       make(chan struct{}),
	}
	if enabled {
		r.epoch = 1
		r.sendCtx, r.sendCancel = context.WithCancel(rctx)
		r.sampler.Prime()
	}
	go r.senderLoop()
	go r.run(rctx)
	return r
}

func (r *Reporter) now() time.Time {
	return r.nowFn()
}

// SetEnabled updates whether periodic and exit reporting is active.
// Disabling cancels in-flight IPC, clears queued samples and pending visibility transitions,
// stops the sampling timer immediately, and resets the baseline.
func (r *Reporter) SetEnabled(enabled bool) {
	if r == nil {
		return
	}
	r.mu.Lock()
	if r.stopped || r.enabled == enabled {
		r.mu.Unlock()
		return
	}
	r.enabled = enabled
	r.epoch++
	if r.sendCancel != nil {
		r.sendCancel()
		r.sendCancel = nil
		r.sendCtx = nil
	}
	r.samplerMu.Lock()
	r.sampler.Reset()
	if enabled {
		r.sampler.Prime()
		r.sendCtx, r.sendCancel = context.WithCancel(r.rootCtx)
	}
	r.samplerMu.Unlock()

	r.stateMu.Lock()
	r.appState = r.uiState
	r.stateEvents = nil
	r.stateOverflow = false
	r.stateMu.Unlock()
	r.baseTime = r.now()

	r.mu.Unlock()
	r.pokeRun()
}

// SetAppState enqueues a timestamped visibility transition without calling OS resource
// probes, acquiring probe/IPC locks, or blocking on HTTP IPC.
func (r *Reporter) SetAppState(state string) {
	if r == nil || (state != "foreground" && state != "background") {
		return
	}
	now := r.now()
	r.stateMu.Lock()
	if r.uiClosed || r.uiState == state {
		r.stateMu.Unlock()
		return
	}
	ev := stateTransition{prev: r.uiState, next: state, at: now}
	r.uiState = state
	if len(r.stateEvents) >= stateQueueCap {
		r.stateOverflow = true
		copy(r.stateEvents, r.stateEvents[1:])
		r.stateEvents[len(r.stateEvents)-1] = ev
	} else {
		r.stateEvents = append(r.stateEvents, ev)
	}
	r.stateMu.Unlock()
	r.pokeRun()
}

func (r *Reporter) popStateEvents() ([]stateTransition, bool) {
	r.stateMu.Lock()
	overflow := r.stateOverflow
	r.stateOverflow = false
	if len(r.stateEvents) == 0 {
		r.stateMu.Unlock()
		return nil, overflow
	}
	events := make([]stateTransition, len(r.stateEvents))
	copy(events, r.stateEvents)
	r.stateEvents = r.stateEvents[:0]
	r.stateMu.Unlock()
	return events, overflow
}

// popStateEventsUpTo drains any transitions captured on or before cutoff while leaving later
// events queued for the subsequent window.
func (r *Reporter) popStateEventsUpTo(cutoff time.Time) ([]stateTransition, bool) {
	r.stateMu.Lock()
	defer r.stateMu.Unlock()
	overflow := r.stateOverflow
	r.stateOverflow = false
	if len(r.stateEvents) == 0 {
		return nil, overflow
	}
	split := 0
	for split < len(r.stateEvents) && !r.stateEvents[split].at.After(cutoff) {
		split++
	}
	if split == 0 {
		return nil, overflow
	}
	taken := make([]stateTransition, split)
	copy(taken, r.stateEvents[:split])
	remaining := copy(r.stateEvents, r.stateEvents[split:])
	r.stateEvents = r.stateEvents[:remaining]
	return taken, overflow
}

func (r *Reporter) pokeRun() {
	select {
	case r.notify <- struct{}{}:
	default:
	}
}

func (r *Reporter) enqueueLocked(epoch uint64, ctx context.Context, sample observability.ResourceSample) {
	if ctx == nil || ctx.Err() != nil {
		return
	}
	item := sendItem{epoch: epoch, ctx: ctx, sample: sample}
	select {
	case r.sendCh <- item:
	default:
		select {
		case <-r.sendCh:
		default:
		}
		select {
		case r.sendCh <- item:
		default:
		}
	}
}

func spillThreshold(prevDur time.Duration) time.Duration {
	if prevDur <= 0 {
		return minSpillLimit
	}
	limit := prevDur / 10
	if limit < minSpillLimit {
		return minSpillLimit
	}
	if limit > maxSpillLimit {
		return maxSpillLimit
	}
	return limit
}

// resolveWindowState decides whether cumulative counters measured from baseAt to probeAt
// cleanly belong to one state or must be discarded due to ambiguity/overflow (Decision C1).
func (r *Reporter) resolveWindowState(events []stateTransition, overflow bool, baseAt, probeAt time.Time) (label string, pure bool) {
	if overflow || len(events) >= 2 {
		return "", false
	}
	if len(events) == 0 {
		return r.appState, true
	}
	ev := events[0]
	prevDur := ev.at.Sub(baseAt)
	spill := probeAt.Sub(ev.at)
	if prevDur <= 0 || spill < 0 || spill > spillThreshold(prevDur) {
		return "", false
	}
	return ev.prev, true
}

func (r *Reporter) sampleAndSettleLocked(events []stateTransition, overflow bool) {
	if r.enabled && r.sendCtx != nil {
		r.samplerMu.Lock()
		d := r.sampler.Sample(r.interval)
		r.samplerMu.Unlock()
		sampleAt := d.EndAt
		if sampleAt.IsZero() {
			sampleAt = r.now()
		}
		baseAt := r.baseTime
		if !d.StartAt.IsZero() && d.StartAt.After(baseAt) {
			baseAt = d.StartAt
		}
		if extraEvents, extraOverflow := r.popStateEventsUpTo(sampleAt); len(extraEvents) > 0 || extraOverflow {
			events = append(events, extraEvents...)
			overflow = overflow || extraOverflow
		}
		// Keep only events at or before the OS snapshot cutoff; any event after sampleAt
		// belongs to the next delta window so it is pushed back or stays queued cleanly.
		label, pure := r.resolveWindowState(events, overflow, baseAt, sampleAt)
		if pure && d.HasWindow && d.WindowSeconds > 0 {
			r.enqueueLocked(r.epoch, r.sendCtx, r.buildSample(d, label))
		}
		r.baseTime = sampleAt
	} else {
		r.baseTime = r.now()
	}
	if len(events) > 0 {
		r.appState = events[len(events)-1].next
	}
}

// Close stops periodic sampling and, if enabled and the remaining window is pure and at
// least 10 seconds, emits one final bounded sample before returning.
func (r *Reporter) Close() {
	if r == nil {
		return
	}
	r.mu.Lock()
	if r.stopped {
		r.mu.Unlock()
		return
	}
	r.stopped = true
	cancelSend := r.sendCancel
	r.sendCancel = nil
	r.sendCtx = nil
	wasEnabled := r.enabled

	r.stateMu.Lock()
	r.uiClosed = true
	pending := make([]stateTransition, len(r.stateEvents))
	copy(pending, r.stateEvents)
	overflow := r.stateOverflow
	r.stateEvents = nil
	r.stateOverflow = false
	r.stateMu.Unlock()

	var exitSample *observability.ResourceSample
	if wasEnabled {
		r.samplerMu.Lock()
		d := r.sampler.Sample(r.interval)
		r.sampler.Reset()
		r.samplerMu.Unlock()
		sampleAt := d.EndAt
		if sampleAt.IsZero() {
			sampleAt = r.now()
		}
		baseAt := r.baseTime
		if !d.StartAt.IsZero() && d.StartAt.After(baseAt) {
			baseAt = d.StartAt
		}
		label, pure := r.resolveWindowState(pending, overflow, baseAt, sampleAt)
		if pure && d.HasWindow && d.WindowSeconds >= minExitWindow {
			s := r.buildSample(d, label)
			exitSample = &s
		}
	}
	r.epoch++
	r.mu.Unlock()

	if cancelSend != nil {
		cancelSend()
	}
	r.cancel()
	close(r.senderStop)
	<-r.done
	<-r.senderDone

	if exitSample != nil {
		ctx, cancel := context.WithTimeout(context.Background(), sendTimeout)
		defer cancel()
		_ = r.client.SendResourceSample(ctx, *exitSample)
	}
}

func (r *Reporter) isEpochActive(epoch uint64) bool {
	r.mu.Lock()
	ok := !r.stopped && r.enabled && r.epoch == epoch
	r.mu.Unlock()
	return ok
}

func (r *Reporter) senderLoop() {
	defer close(r.senderDone)
	for {
		select {
		case <-r.senderStop:
			return
		case item := <-r.sendCh:
			if !r.isEpochActive(item.epoch) || item.ctx.Err() != nil {
				continue
			}
			reqCtx, cancel := context.WithTimeout(item.ctx, sendTimeout)
			_ = r.client.SendResourceSample(reqCtx, item.sample)
			cancel()
		}
	}
}

func (r *Reporter) run(ctx context.Context) {
	defer close(r.done)
	timer := time.NewTimer(time.Hour)
	timer.Stop()
	defer timer.Stop()

	var tick <-chan time.Time
	arm := func(on bool) {
		if !timer.Stop() {
			select {
			case <-timer.C:
			default:
			}
		}
		if on {
			timer.Reset(r.interval)
			tick = timer.C
		} else {
			tick = nil
		}
	}

	r.mu.Lock()
	active := r.enabled
	activeEpoch := r.epoch
	r.mu.Unlock()
	if active {
		arm(true)
	}

	for {
		select {
		case <-ctx.Done():
			return
		case <-r.notify:
			events, overflow := r.popStateEvents()
			r.mu.Lock()
			if r.stopped {
				r.mu.Unlock()
				arm(false)
				continue
			}
			if len(events) > 0 || overflow {
				r.sampleAndSettleLocked(events, overflow)
			}
			active = r.enabled
			activeEpoch = r.epoch
			r.mu.Unlock()
			arm(active)
		case <-tick:
			tick = nil
			events, overflow := r.popStateEvents()
			r.mu.Lock()
			if r.stopped {
				r.mu.Unlock()
				arm(false)
				continue
			}
			if len(events) > 0 || overflow {
				r.sampleAndSettleLocked(events, overflow)
				select {
				case <-r.notify:
				default:
				}
				active = r.enabled
				activeEpoch = r.epoch
				r.mu.Unlock()
				arm(active)
				continue
			}
			if r.enabled && r.epoch == activeEpoch && r.sendCtx != nil {
				r.sampleAndSettleLocked(nil, false)
				select {
				case <-r.notify:
				default:
				}
				r.mu.Unlock()
				arm(true)
			} else {
				active = r.enabled
				activeEpoch = r.epoch
				r.mu.Unlock()
				arm(active)
			}
		}
	}
}

func (r *Reporter) buildSample(d procmetrics.Delta, state string) observability.ResourceSample {
	if state != "foreground" {
		state = "background"
	}
	return observability.ResourceSample{
		ProcessRole:           r.role,
		AppState:              state,
		PowerSource:           "unknown",
		WindowSeconds:         d.WindowSeconds,
		CPUSeconds:            d.CPUSeconds,
		CPUProbeStatus:        string(d.CPUStatus),
		SleepJumped:           d.SleepJumped,
		MemoryFootprintBytes:  d.MemoryFootprintBytes,
		MemoryProbeStatus:     string(d.MemoryStatus),
		GoHeapBytes:           d.GoHeapBytes,
		Wakeups:               d.Wakeups,
		WakeupsProbeStatus:    string(d.WakeupsStatus),
		ContextSwitches:       d.ContextSwitches,
		ContextSwitchesStatus: string(d.ContextSwitchesStatus),
	}
}
