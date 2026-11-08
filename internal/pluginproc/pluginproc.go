// Package pluginproc runs an out-of-process Go plugin and presents it to the
// engine as an ordinary plugin.Plugin / Processor / Actor. It owns spawning,
// the handshake, deadlines, termination and bounded restarts; the wire
// protocol lives in plugin/rpc.
package pluginproc

import (
	"bytes"
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"os/exec"
	"slices"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Spec describes the process to run.
type Spec struct {
	Path   string   // absolute path of the executable
	SHA256 string   // hex digest the file must match; required
	Args   []string // extra arguments
	Env    []string // extra KEY=VALUE entries; nothing else is inherited
	Dir    string   // private data directory, also the working directory
	// Expected is the identity the process must present in the handshake.
	Expected plugin.Spec
	// Events a processor subscribes to; empty means all it supports.
	Events []string
}

// Options tune timing; zero values use the contract defaults.
type Options struct {
	// Notify receives host.notify messages (already bounded and rate limited).
	Notify         func(message string)
	ProcessTimeout time.Duration // 200ms
	ActionTimeout  time.Duration // 10s
	StopTimeout    time.Duration // 5s
	CancelGrace    time.Duration // 1s
	StartTimeout   time.Duration // 5s handshake
	Backoff        []time.Duration
	MaxRestarts    int           // 3
	RestartWindow  time.Duration // 60s
	// MaxActionBytes caps an action result (default rpc.MaxActionLen).
	MaxActionBytes int
}

func (o *Options) fill() {
	def := func(d *time.Duration, v time.Duration) {
		if *d <= 0 {
			*d = v
		}
	}
	def(&o.ProcessTimeout, 200*time.Millisecond)
	def(&o.ActionTimeout, 10*time.Second)
	def(&o.StopTimeout, 5*time.Second)
	def(&o.CancelGrace, time.Second)
	def(&o.StartTimeout, rpc.DefaultHandshakeTimeout)
	def(&o.RestartWindow, 60*time.Second)
	if len(o.Backoff) == 0 {
		o.Backoff = []time.Duration{time.Second, 2 * time.Second, 4 * time.Second}
	}
	if o.MaxRestarts <= 0 {
		o.MaxRestarts = 3
	}
	if o.MaxActionBytes <= 0 || o.MaxActionBytes > rpc.MaxActionLen {
		o.MaxActionBytes = rpc.MaxActionLen
	}
}

const (
	stNew = iota
	stRunning
	stRestarting
	stUnavailable
	stClosed
)

type session struct {
	cmd     *exec.Cmd
	conn    *rpc.Conn
	exited  chan struct{}
	actions sync.Map // run_action request ID -> *collector
}

// Proxy is the engine-facing side of one plugin process.
type Proxy struct {
	spec Spec
	opts Options
	host plugin.Host
	cfg  plugin.Config

	gate chan struct{} // Process/Run one at a time
	gen  atomic.Uint64
	stop chan struct{}
	wg   sync.WaitGroup

	mu      sync.Mutex
	state   int
	sess    *session
	crashes []time.Time

	notifyMu   sync.Mutex
	notifyTime []time.Time
}

// ErrUnavailable is returned while the process is down or has exhausted its
// restart budget.
var ErrUnavailable = rpc.NewError(rpc.CodeUnavailable)

// New returns a plugin whose concrete type matches the capabilities in
// spec.Expected, so engine capability checks see the right interfaces.
func New(spec Spec, opts Options) plugin.Plugin {
	opts.fill()
	p := &Proxy{spec: spec, opts: opts, gate: make(chan struct{}, 1), stop: make(chan struct{})}
	proc := slices.Contains(spec.Expected.Capabilities, plugin.CapProcessor)
	act := slices.Contains(spec.Expected.Capabilities, plugin.CapAction)
	switch {
	case proc && act:
		return both{p}
	case proc:
		return processorOnly{p}
	case act:
		return actorOnly{p}
	}
	return p
}

type processorOnly struct{ *Proxy }
type actorOnly struct{ *Proxy }
type both struct{ *Proxy }

func (p processorOnly) Process(ctx context.Context, c plugin.ProcessContext) error {
	return p.process(ctx, c)
}
func (p actorOnly) Run(ctx context.Context, c plugin.ActionContext) error { return p.run(ctx, c) }
func (p both) Process(ctx context.Context, c plugin.ProcessContext) error { return p.process(ctx, c) }
func (p both) Run(ctx context.Context, c plugin.ActionContext) error      { return p.run(ctx, c) }

func (p *Proxy) Spec() plugin.Spec { return p.spec.Expected }

func wireSpec(s plugin.Spec) rpc.Spec {
	out := rpc.Spec{ID: s.ID, Version: s.Version, APIVersion: s.APIVersion, Capabilities: slices.Clone(s.Capabilities)}
	for _, a := range s.Actions {
		out.Actions = append(out.Actions, rpc.ActionSpec{ID: a.ID, Title: a.Title, Accepts: slices.Clone(a.Accepts)})
	}
	return out
}

func verifyDigest(path, want string) error {
	wantB, err := hex.DecodeString(want)
	if err != nil || len(wantB) != sha256.Size {
		return errors.New("pluginproc: SHA256 must be 64 hex digits")
	}
	f, err := os.Open(path)
	if err != nil {
		return errors.New("pluginproc: cannot open the plugin executable")
	}
	defer f.Close()
	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return errors.New("pluginproc: cannot read the plugin executable")
	}
	if subtle.ConstantTimeCompare(h.Sum(nil), wantB) != 1 {
		return errors.New("pluginproc: executable digest mismatch")
	}
	return nil
}

// spawn starts the process and completes the handshake.
func (p *Proxy) spawn(ctx context.Context) (*session, error) {
	if err := verifyDigest(p.spec.Path, p.spec.SHA256); err != nil {
		return nil, err
	}
	cmd := exec.Command(p.spec.Path, p.spec.Args...)
	cmd.Dir = p.spec.Dir
	cmd.Env = append([]string{"UNIC_PLUGIN_PROTOCOL=1"}, p.spec.Env...)
	if p.spec.Dir != "" {
		cmd.Env = append(cmd.Env, "UNIC_PLUGIN_DIR="+p.spec.Dir)
	}
	cmd.Stderr = nil // discarded: plugin diagnostics must never reach host logs
	setProcAttrs(cmd)
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, errors.New("pluginproc: cannot start the plugin")
	}
	s := &session{cmd: cmd, exited: make(chan struct{})}
	go func() {
		_ = cmd.Wait()
		close(s.exited)
	}()
	rwc := rpc.NewDuplex(stdout, stdin, stdout, stdin)
	lim, err := rpc.HostHandshake(ctx, rwc, wireSpec(p.spec.Expected), rpc.DefaultLimits, p.opts.StartTimeout)
	if err != nil {
		p.terminate(s)
		return nil, err
	}
	s.conn = rpc.NewConn(rwc, rpc.Options{
		Role: rpc.RoleEngine, Limits: lim, CancelGrace: p.opts.CancelGrace,
		Handler: func(ctx context.Context, c *rpc.Call) (any, error) { return p.handle(s, ctx, c) },
		OnStuck: func() { killTree(cmd) },
	})
	go p.watch(s)
	return s, nil
}

// terminate kills the process tree and waits for it.
func (p *Proxy) terminate(s *session) {
	killTree(s.cmd)
	if s.conn != nil {
		s.conn.Close()
	}
	<-s.exited
}

func (p *Proxy) watch(s *session) {
	<-s.exited
	s.conn.Close()
	p.onExit(s)
}

func (p *Proxy) handshakeCalls(ctx context.Context, s *session, cfg plugin.Config) error {
	events := p.spec.Events
	if events == nil {
		events = []string{}
	}
	if cfg == nil {
		cfg = plugin.Config{}
	}
	_, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodInit, Timeout: p.opts.StartTimeout, Params: rpc.Init{Config: cfg, Events: events}}, nil)
	return err
}

func (p *Proxy) Init(ctx context.Context, host plugin.Host, cfg plugin.Config) error {
	p.host, p.cfg = host, cfg
	s, err := p.spawn(ctx)
	if err != nil {
		return err
	}
	if err := p.handshakeCalls(ctx, s, cfg); err != nil {
		p.terminate(s)
		return err
	}
	p.mu.Lock()
	p.sess, p.state = s, stRunning
	p.mu.Unlock()
	return nil
}

func (p *Proxy) Start(ctx context.Context) error {
	s := p.current()
	if s == nil {
		return ErrUnavailable
	}
	_, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodStart, Timeout: p.opts.StartTimeout, Params: rpc.Empty{}}, nil)
	return err
}

func (p *Proxy) current() *session {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.state != stRunning {
		return nil
	}
	return p.sess
}

// Close asks the plugin to stop, then ends the process tree within the stop
// window. It is idempotent and safe on a partly initialized plugin.
func (p *Proxy) Close(ctx context.Context) error {
	p.mu.Lock()
	if p.state == stClosed {
		p.mu.Unlock()
		return nil
	}
	p.state = stClosed
	s := p.sess
	p.sess = nil
	close(p.stop)
	p.mu.Unlock()

	if s != nil {
		cctx, cancel := context.WithTimeout(context.WithoutCancel(ctx), p.opts.StopTimeout)
		defer cancel()
		closeCtx, cancelClose := context.WithTimeout(cctx, p.opts.StopTimeout/2)
		_, _ = s.conn.Call(closeCtx, rpc.CallOptions{Method: rpc.MethodClose, Timeout: p.opts.StopTimeout / 2, Params: rpc.Empty{}}, nil)
		cancelClose()
		s.conn.Close() // EOF on the plugin's stdin asks it to exit
		select {
		case <-s.exited:
		case <-time.After(max(time.Until(deadlineOf(cctx)), 0)):
			killTree(s.cmd)
			<-s.exited
		}
	}
	p.wg.Wait()
	return nil
}

func deadlineOf(ctx context.Context) time.Time {
	d, _ := ctx.Deadline()
	return d
}

// onExit handles an unexpected end of the process: plan a bounded restart.
func (p *Proxy) onExit(s *session) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.state == stClosed || p.sess != s {
		return
	}
	p.sess = nil
	p.failLocked()
}

// failLocked records a crash and either schedules a restart or gives up.
func (p *Proxy) failLocked() {
	now := time.Now()
	p.crashes = append(slices.DeleteFunc(p.crashes, func(t time.Time) bool { return now.Sub(t) > p.opts.RestartWindow }), now)
	if len(p.crashes) > p.opts.MaxRestarts {
		p.state = stUnavailable
		return
	}
	p.state = stRestarting
	delay := p.opts.Backoff[min(len(p.crashes)-1, len(p.opts.Backoff)-1)]
	p.wg.Add(1)
	go p.restart(delay)
}

func (p *Proxy) restart(delay time.Duration) {
	defer p.wg.Done()
	select {
	case <-time.After(delay):
	case <-p.stop:
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*p.opts.StartTimeout)
	defer cancel()
	go func() {
		select {
		case <-p.stop:
			cancel()
		case <-ctx.Done():
		}
	}()
	s, err := p.spawn(ctx)
	if err == nil {
		if err = p.handshakeCalls(ctx, s, p.cfg); err == nil {
			_, err = s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodStart, Timeout: p.opts.StartTimeout, Params: rpc.Empty{}}, nil)
		}
		if err != nil {
			p.terminate(s)
		}
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.state == stClosed {
		if err == nil {
			go p.terminate(s)
		}
		return
	}
	if err != nil {
		p.failLocked()
		return
	}
	p.sess, p.state = s, stRunning
}

// handle serves requests the plugin makes of the engine.
func (p *Proxy) handle(s *session, ctx context.Context, c *rpc.Call) (any, error) {
	switch c.Method {
	case rpc.MethodNotify:
		var n rpc.Notify
		if err := rpc.Decode(c.Params, &n); err != nil || c.Parent == 0 || len(n.Message) > 1024 || !utf8.ValidString(n.Message) {
			return nil, rpc.NewError(rpc.CodeInvalidArgument)
		}
		p.notify(n.Message)
		return nil, nil
	case rpc.MethodStream:
		return p.openActionStream(s, c)
	}
	return nil, rpc.NewError(rpc.CodeUnsupported)
}

func (p *Proxy) notify(msg string) {
	if p.opts.Notify == nil && p.host == nil {
		return
	}
	p.notifyMu.Lock()
	now := time.Now()
	p.notifyTime = slices.DeleteFunc(p.notifyTime, func(t time.Time) bool { return now.Sub(t) >= time.Second })
	ok := len(p.notifyTime) < 5
	if ok {
		p.notifyTime = append(p.notifyTime, now)
	}
	p.notifyMu.Unlock()
	switch {
	case !ok:
	case p.opts.Notify != nil:
		p.opts.Notify(msg)
	default:
		p.host.Notify(context.Background(), msg)
	}
}

// collector buffers an action's text stream outside the connection so credit
// keeps flowing while the response is still in transit.
type collector struct {
	mu     sync.Mutex
	opened bool
	id     uint64
	size   uint64
	done   chan struct{}
	buf    bytes.Buffer
	err    error
}

func (p *Proxy) openActionStream(s *session, c *rpc.Call) (any, error) {
	var d rpc.StreamDesc
	if err := rpc.Decode(c.Params, &d); err != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	v, ok := s.actions.Load(c.Parent)
	if !ok || d.Purpose != rpc.PurposeActionText {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	col := v.(*collector)
	col.mu.Lock()
	if col.opened {
		col.mu.Unlock()
		return nil, rpc.NewError("conflict")
	}
	col.opened = true
	col.mu.Unlock()
	rs, err := c.Conn.Expect(c.Conn.Context(), c.Parent, d, uint64(p.opts.MaxActionBytes))
	if err != nil {
		return nil, err
	}
	col.mu.Lock()
	col.id, col.size = uint64(d.StreamID), uint64(d.Size)
	col.mu.Unlock()
	go func() {
		_, err := io.Copy(&col.buf, rs)
		col.mu.Lock()
		col.err = err
		col.mu.Unlock()
		close(col.done)
	}()
	return nil, nil
}

func (p *Proxy) callTimeout(ctx context.Context, def time.Duration) time.Duration {
	if dl, ok := ctx.Deadline(); ok {
		return min(def, time.Until(dl))
	}
	return def
}

func (p *Proxy) enter(ctx context.Context) error {
	select {
	case p.gate <- struct{}{}:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (p *Proxy) leave() { <-p.gate }

func wireItem(it plugin.Item, textDesc *rpc.StreamDesc) rpc.Item {
	out := rpc.Item{ID: it.ID, Type: it.Type, Text: textDesc, Formats: slices.Clone(it.Formats),
		Origin: rpc.Origin{Kind: it.Origin.Kind, DeviceID: it.Origin.DeviceID, PluginID: it.Origin.PluginID}, Files: []rpc.File{}}
	if out.Formats == nil {
		out.Formats = []string{}
	}
	for _, f := range it.Files {
		out.Files = append(out.Files, rpc.File{Name: f.Name, Size: rpc.U64(f.Size)})
	}
	return out
}

// sendText announces and streams the item's text as the call's input.
func (p *Proxy) sendText(s *session, id uint64, text string) (*rpc.StreamDesc, error) {
	if text == "" {
		return nil, nil
	}
	ss, d, err := s.conn.NewSend(s.conn.Context(), id, rpc.PurposeProcessText, uint64(len(text)))
	if err != nil {
		return nil, err
	}
	go func() {
		if _, err := io.WriteString(ss, text); err == nil {
			_ = ss.Close()
		}
	}()
	return &d, nil
}

func (p *Proxy) process(ctx context.Context, pc plugin.ProcessContext) error {
	s := p.current()
	if s == nil {
		return ErrUnavailable
	}
	if err := p.enter(ctx); err != nil {
		return err
	}
	defer p.leave()

	item, pol := pc.Item(), pc.Policy()
	gen := p.gen.Add(1)
	var expires rpc.U64
	if pol.ExpireAfter > 0 {
		expires = rpc.U64(time.Now().Add(pol.ExpireAfter).UnixMilli())
	}
	wp := rpc.Policy{Persist: pol.Persist, Sync: pol.Sync, ExpiresAtMs: expires, Tags: slices.Clone(pol.Tags)}
	if wp.Tags == nil {
		wp.Tags = []string{}
	}
	var patch rpc.Patch
	id, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodProcess, Timeout: p.callTimeout(ctx, p.opts.ProcessTimeout),
		Build: func(id uint64) (any, error) {
			d, err := p.sendText(s, id, item.Text)
			if err != nil {
				return nil, err
			}
			return rpc.Process{Event: pc.Event(), Generation: rpc.U64(gen), Item: wireItem(item, d), Policy: wp}, nil
		}}, &patch)
	if err != nil {
		return err
	}
	defer s.conn.ReleaseRoot(id)
	return applyPatch(patch, gen, pc, pol.Tags)
}

func applyPatch(patch rpc.Patch, gen uint64, pc plugin.ProcessContext, current []string) error {
	if uint64(patch.Generation) != gen {
		return rpc.NewError(rpc.CodeInvalidState)
	}
	if err := patch.Validate(); err != nil {
		return err
	}
	if patch.Tags != nil {
		for _, t := range current {
			if !slices.Contains(*patch.Tags, t) {
				if err := pc.RemoveTag(t); err != nil {
					return err
				}
			}
		}
		for _, t := range *patch.Tags {
			if !slices.Contains(current, t) {
				if err := pc.AddTag(t); err != nil {
					return err
				}
			}
		}
	}
	if patch.Persist != nil {
		if err := pc.SetPersist(*patch.Persist); err != nil {
			return err
		}
	}
	if patch.Sync != nil {
		if err := pc.SetSync(*patch.Sync); err != nil {
			return err
		}
	}
	if patch.ExpireAfterMs != nil {
		if err := pc.SetExpire(time.Duration(*patch.ExpireAfterMs) * time.Millisecond); err != nil {
			return err
		}
	}
	return nil
}

func (p *Proxy) run(ctx context.Context, ac plugin.ActionContext) error {
	s := p.current()
	if s == nil {
		return ErrUnavailable
	}
	if err := p.enter(ctx); err != nil {
		return err
	}
	defer p.leave()

	item := ac.Item()
	gen := p.gen.Add(1)
	col := &collector{done: make(chan struct{})}
	var res rpc.ActionResult
	id, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodRunAction, Timeout: p.callTimeout(ctx, p.opts.ActionTimeout),
		Build: func(id uint64) (any, error) {
			s.actions.Store(id, col)
			d, err := p.sendText(s, id, item.Text)
			if err != nil {
				return nil, err
			}
			return rpc.ActionRun{ActionID: ac.Action(), Generation: rpc.U64(gen), Item: wireItem(item, d)}, nil
		}}, &res)
	defer s.actions.Delete(id)
	if err != nil {
		return err
	}
	defer s.conn.ReleaseRoot(id)
	col.mu.Lock()
	opened, sid, size := col.opened, col.id, col.size
	col.mu.Unlock()
	if uint64(res.Generation) != gen || !opened || uint64(res.Text.StreamID) != sid || uint64(res.Text.Size) != size {
		return rpc.NewError(rpc.CodeInvalidState)
	}
	select {
	case <-col.done:
	case <-ctx.Done():
		return ctx.Err()
	}
	col.mu.Lock()
	defer col.mu.Unlock()
	if col.err != nil || uint64(col.buf.Len()) != size || size == 0 || !utf8.Valid(col.buf.Bytes()) {
		return rpc.NewError(rpc.CodeInvalidState)
	}
	return ac.SetText(col.buf.String())
}
