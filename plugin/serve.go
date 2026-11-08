package plugin

import (
	"context"
	"errors"
	"io"
	"os"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/plugin/rpc"
)

// Errors a plugin may return to be reported with a stable code. Any other
// error is reported as "internal"; its text never leaves the process.
var (
	ErrInvalidConfig   = errors.New("plugin: invalid config")
	ErrInvalidArgument = errors.New("plugin: invalid argument")
)

// MaxTextBytes bounds the text a plugin receives or returns.
const MaxTextBytes = rpc.MaxActionLen

// Serve runs p as an out-of-process plugin over stdin/stdout until the host
// closes the pipe. stdout belongs to the protocol: never print to it.
func Serve(p Plugin) error { return ServeIO(context.Background(), p, os.Stdin, os.Stdout) }

// ServeIO is Serve over explicit streams.
func ServeIO(ctx context.Context, p Plugin, r io.Reader, w io.Writer) error {
	s := &server{p: p, spec: p.Spec(), gate: make(chan struct{}, 1)}
	ws := wireSpec(s.spec)
	rw := rpc.NewDuplex(r, w)
	lim, err := rpc.PluginHandshake(rw, ws, rpc.DefaultLimits)
	if err != nil {
		return err
	}
	var closers []io.Closer
	if c, ok := r.(io.Closer); ok {
		closers = append(closers, c)
	}
	if c, ok := w.(io.Closer); ok {
		closers = append(closers, c)
	}
	s.conn = rpc.NewConn(rpc.NewDuplex(r, w, closers...), rpc.Options{Role: rpc.RolePlugin, Limits: lim, Handler: s.handle})
	select {
	case <-s.conn.Done():
	case <-ctx.Done():
		s.conn.Close()
	}
	// The host went away: cancel what is running and release the plugin.
	cctx, cancel := context.WithTimeout(context.WithoutCancel(ctx), 3*time.Second)
	defer cancel()
	s.closePlugin(cctx)
	return nil
}

func wireSpec(s Spec) rpc.Spec {
	out := rpc.Spec{ID: s.ID, Version: s.Version, APIVersion: s.APIVersion, Capabilities: slices.Clone(s.Capabilities)}
	for _, a := range s.Actions {
		out.Actions = append(out.Actions, rpc.ActionSpec{ID: a.ID, Title: a.Title, Accepts: slices.Clone(a.Accepts)})
	}
	return out
}

type callKey struct{}

type server struct {
	p    Plugin
	spec Spec
	conn *rpc.Conn
	gate chan struct{} // Process/Run run one at a time

	mu      sync.Mutex
	state   int // 0 new, 1 initialized, 2 started, 3 closed
	events  []string
	closing bool

	notifyMu   sync.Mutex
	notifyTime []time.Time
}

func (s *server) handle(ctx context.Context, c *rpc.Call) (any, error) {
	ctx = context.WithValue(ctx, callKey{}, c.ID)
	var res any
	var err error
	switch c.Method {
	case rpc.MethodInit:
		res, err = s.init(ctx, c)
	case rpc.MethodStart:
		res, err = s.start(ctx)
	case rpc.MethodClose:
		s.closePlugin(ctx)
		return nil, nil
	case rpc.MethodProcess:
		res, err = s.process(ctx, c)
	case rpc.MethodRunAction:
		res, err = s.runAction(ctx, c)
	default:
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	return res, mapError(ctx, err)
}

func mapError(ctx context.Context, err error) error {
	var re *rpc.Error
	switch {
	case err == nil:
		return nil
	case errors.As(err, &re):
		return re
	case errors.Is(err, ErrInvalidConfig):
		return rpc.NewError(rpc.CodeInvalidConfig)
	case errors.Is(err, ErrInvalidArgument):
		return rpc.NewError(rpc.CodeInvalidArgument)
	case errors.Is(err, context.DeadlineExceeded) || errors.Is(ctx.Err(), context.DeadlineExceeded):
		return rpc.NewError(rpc.CodeTimeout)
	case errors.Is(err, context.Canceled):
		return rpc.NewError(rpc.CodeCancelled)
	}
	return rpc.NewError(rpc.CodeInternal)
}

// safely runs fn, turning a panic into plugin_panic.
func safely(fn func() error) (err error) {
	defer func() {
		if recover() != nil {
			err = rpc.NewError(rpc.CodePanic)
		}
	}()
	return fn()
}

func (s *server) init(ctx context.Context, c *rpc.Call) (any, error) {
	var in rpc.Init
	if err := rpc.Decode(c.Params, &in); err != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	s.mu.Lock()
	if s.state != 0 {
		s.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeInvalidState)
	}
	s.state = 1
	s.events = in.Events
	s.mu.Unlock()
	return nil, safely(func() error { return s.p.Init(ctx, &remoteHost{s: s}, Config(in.Config)) })
}

func (s *server) start(ctx context.Context) (any, error) {
	s.mu.Lock()
	if s.state != 1 {
		s.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeInvalidState)
	}
	s.state = 2
	s.mu.Unlock()
	return nil, safely(func() error { return s.p.Start(ctx) })
}

func (s *server) closePlugin(ctx context.Context) {
	s.mu.Lock()
	if s.state == 3 || s.closing {
		s.mu.Unlock()
		return
	}
	s.state = 3
	s.closing = true
	s.mu.Unlock()
	_ = safely(func() error { return s.p.Close(ctx) })
}

func (s *server) started() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.state != 2 {
		return rpc.NewError(rpc.CodeInvalidState)
	}
	return nil
}

// readText drains the text stream announced with an item, if any.
func (s *server) readText(ctx context.Context, c *rpc.Call, d *rpc.StreamDesc) (string, error) {
	if d == nil {
		return "", nil
	}
	rs, err := c.Conn.Expect(ctx, c.ID, *d, MaxTextBytes)
	if err != nil {
		return "", err
	}
	defer rs.Close()
	b, err := io.ReadAll(rs)
	if err != nil {
		return "", err
	}
	if !utf8.Valid(b) {
		return "", rpc.NewError(rpc.CodeInvalidArgument)
	}
	return string(b), nil
}

func (s *server) enter(ctx context.Context) error {
	select {
	case s.gate <- struct{}{}:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (s *server) leave() { <-s.gate }

func publicItem(i rpc.Item, text string) Item {
	out := Item{ID: i.ID, Type: i.Type, Text: text, Formats: slices.Clone(i.Formats),
		Origin: Origin{Kind: i.Origin.Kind, DeviceID: i.Origin.DeviceID, PluginID: i.Origin.PluginID}}
	for _, f := range i.Files {
		out.Files = append(out.Files, File{Name: f.Name, Size: int64(f.Size)})
	}
	return out
}

func (s *server) process(ctx context.Context, c *rpc.Call) (any, error) {
	proc, ok := s.p.(Processor)
	if !ok {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.Process
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.started(); err != nil {
		return nil, err
	}
	s.mu.Lock()
	subscribed := len(s.events) == 0 || slices.Contains(s.events, in.Event)
	s.mu.Unlock()
	if !subscribed {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	text, err := s.readText(ctx, c, in.Item.Text)
	if err != nil {
		return nil, err
	}
	if err := s.enter(ctx); err != nil {
		return nil, err
	}
	defer s.leave()

	pc := newProcCtx(in, text)
	if err := safely(func() error { return proc.Process(ctx, pc) }); err != nil {
		return nil, err
	}
	if pc.fault != nil {
		return nil, pc.fault
	}
	return pc.patch(in.Generation), nil
}

func (s *server) runAction(ctx context.Context, c *rpc.Call) (any, error) {
	act, ok := s.p.(Actor)
	if !ok {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.ActionRun
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.started(); err != nil {
		return nil, err
	}
	if !slices.ContainsFunc(s.spec.Actions, func(a ActionSpec) bool { return a.ID == in.ActionID }) {
		return nil, rpc.NewError(rpc.CodeNotFound)
	}
	text, err := s.readText(ctx, c, in.Item.Text)
	if err != nil {
		return nil, err
	}
	if err := s.enter(ctx); err != nil {
		return nil, err
	}
	defer s.leave()

	ac := &actionCtx{id: in.ActionID, item: publicItem(in.Item, text)}
	if err := safely(func() error { return act.Run(ctx, ac) }); err != nil {
		return nil, err
	}
	if !ac.set || ac.text == "" || len(ac.text) > MaxTextBytes {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	// Stream the result back; the host accepts it before any byte flows.
	ss, desc, err := c.Conn.NewSend(ctx, c.ID, rpc.PurposeActionText, uint64(len(ac.text)))
	if err != nil {
		return nil, err
	}
	if _, err := c.Conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodStream, Parent: c.ID, Timeout: 5 * time.Second, Params: desc}, nil); err != nil {
		return nil, err
	}
	if _, err := ss.Write([]byte(ac.text)); err != nil {
		return nil, err
	}
	if err := ss.Close(); err != nil {
		return nil, err
	}
	return rpc.ActionResult{Generation: in.Generation, Text: desc}, nil
}

// procCtx implements ProcessContext and records changes as a patch.
type procCtx struct {
	event string
	item  Item
	cur   Policy
	orig  Policy
	fault error

	persistSet, syncSet, expireSet, tagsSet bool
}

func newProcCtx(in rpc.Process, text string) *procCtx {
	pol := Policy{Persist: in.Policy.Persist, Sync: in.Policy.Sync, Tags: slices.Clone(in.Policy.Tags)}
	if ms := int64(in.Policy.ExpiresAtMs); ms > 0 {
		pol.ExpireAfter = max(time.Until(time.UnixMilli(ms)), time.Millisecond)
	}
	return &procCtx{event: in.Event, item: publicItem(in.Item, text), cur: pol, orig: pol}
}

func (p *procCtx) Event() string { return p.event }
func (p *procCtx) Item() Item    { return p.item }
func (p *procCtx) Policy() Policy {
	out := p.cur
	out.Tags = slices.Clone(p.cur.Tags)
	return out
}

func (p *procCtx) fail(err error) error {
	if p.fault == nil {
		p.fault = rpc.NewError(rpc.CodeInvalidArgument)
	}
	return err
}

func validTag(t string) bool { return t != "" && len(t) <= rpc.MaxTagBytes && utf8.ValidString(t) }

func (p *procCtx) AddTag(t string) error {
	if p.fault != nil {
		return ErrInvalidArgument
	}
	if !validTag(t) || (!slices.Contains(p.cur.Tags, t) && len(p.cur.Tags) >= rpc.MaxTags) {
		return p.fail(ErrInvalidArgument)
	}
	if !slices.Contains(p.cur.Tags, t) {
		p.cur.Tags = append(p.cur.Tags, t)
		slices.Sort(p.cur.Tags)
		p.tagsSet = true
	}
	return nil
}

func (p *procCtx) RemoveTag(t string) error {
	if p.fault != nil {
		return ErrInvalidArgument
	}
	if !validTag(t) {
		return p.fail(ErrInvalidArgument)
	}
	if i := slices.Index(p.cur.Tags, t); i >= 0 {
		p.cur.Tags = slices.Delete(p.cur.Tags, i, i+1)
		p.tagsSet = true
	}
	return nil
}

func (p *procCtx) SetPersist(v bool) error {
	if p.fault != nil {
		return ErrInvalidArgument
	}
	p.cur.Persist, p.persistSet = v, true
	return nil
}

func (p *procCtx) SetSync(v bool) error {
	if p.fault != nil {
		return ErrInvalidArgument
	}
	p.cur.Sync, p.syncSet = v, true
	return nil
}

func (p *procCtx) SetExpire(d time.Duration) error {
	if p.fault != nil {
		return ErrInvalidArgument
	}
	if d <= 0 || d%time.Millisecond != 0 {
		return p.fail(ErrInvalidArgument)
	}
	p.cur.ExpireAfter, p.expireSet = d, true
	return nil
}

func (p *procCtx) patch(gen rpc.U64) rpc.Patch {
	out := rpc.Patch{Generation: gen}
	if p.persistSet {
		out.Persist = &p.cur.Persist
	}
	if p.syncSet {
		out.Sync = &p.cur.Sync
	}
	if p.expireSet {
		ms := rpc.U64(p.cur.ExpireAfter / time.Millisecond)
		out.ExpireAfterMs = &ms
	}
	if p.tagsSet {
		tags := slices.Clone(p.cur.Tags)
		if tags == nil {
			tags = []string{}
		}
		out.Tags = &tags
	}
	return out
}

type actionCtx struct {
	id   string
	item Item
	text string
	set  bool
}

func (a *actionCtx) Action() string { return a.id }
func (a *actionCtx) Item() Item     { return a.item }
func (a *actionCtx) SetText(s string) error {
	if a.set || s == "" || len(s) > MaxTextBytes || !utf8.ValidString(s) {
		return ErrInvalidArgument
	}
	a.text, a.set = s, true
	return nil
}

// remoteHost forwards Notify to the engine within the active callback.
type remoteHost struct{ s *server }

func (h *remoteHost) Notify(ctx context.Context, message string) {
	parent, ok := ctx.Value(callKey{}).(uint64)
	if !ok {
		return
	}
	s := h.s
	s.notifyMu.Lock()
	now := time.Now()
	s.notifyTime = slices.DeleteFunc(s.notifyTime, func(t time.Time) bool { return now.Sub(t) >= time.Second })
	if len(s.notifyTime) >= 5 {
		s.notifyMu.Unlock()
		return
	}
	s.notifyTime = append(s.notifyTime, now)
	s.notifyMu.Unlock()
	if len(message) > 1024 {
		message = strings.ToValidUTF8(message[:1024], "") // drop a cut-off trailing rune
	}
	if !utf8.ValidString(message) {
		return
	}
	_, _ = s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodNotify, Parent: parent, Timeout: time.Second, Params: rpc.Notify{Message: message}}, nil)
}
