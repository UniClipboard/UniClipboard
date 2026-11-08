package rpc

import (
	"context"
	"errors"
	"io"
	"sync"
	"time"
)

// Role decides which half of the ID space a side allocates: the engine uses
// odd request and stream IDs, the plugin even ones.
type Role int

const (
	RoleEngine Role = iota
	RolePlugin
)

// Call is an incoming request being served.
type Call struct {
	ID     uint64
	Method string
	Parent uint64
	Params []byte
	Conn   *Conn
}

// Handler serves incoming requests. A returned *Error is sent as is; a
// context error becomes timeout/cancelled; anything else becomes internal.
// The result is marshalled as the response object (nil means {}).
type Handler func(ctx context.Context, call *Call) (any, error)

// Options configure a Conn.
type Options struct {
	Role    Role
	Limits  Limits // negotiated
	Handler Handler
	// CancelGrace is how long a cancelled call may stay unanswered before
	// OnStuck is invoked (default 1s).
	CancelGrace time.Duration
	// OnStuck runs when a cancelled call was not acknowledged in time. The
	// engine uses it to terminate the plugin process.
	OnStuck func()
}

type pendingCall struct {
	ch        chan Response
	depth     int
	cancelled bool
	timer     *time.Timer
}

type inflight struct {
	cancel context.CancelFunc
	depth  int
}

// Conn is one multiplexed protocol session.
type Conn struct {
	rwc  io.ReadWriteCloser
	opts Options
	lim  Limits

	ctx    context.Context
	cancel context.CancelFunc

	wmu sync.Mutex
	// reqMu orders request IDs on the wire: the peer requires them to rise.
	reqMu sync.Mutex

	mu         sync.Mutex
	changed    chan struct{}
	nextReq    uint64
	nextStream uint64
	lastPeer   uint64
	pending    map[uint64]*pendingCall
	handlers   map[uint64]*inflight
	recv       map[uint64]*RecvStream
	send       map[uint64]*SendStream
	maxStream  [2]uint64 // highest stream ID seen per parity
	budgetUsed int
	drained    int
	err        error
	done       chan struct{}
	readerDone chan struct{}
}

// NewConn starts the reader for an already-handshaken connection.
func NewConn(rwc io.ReadWriteCloser, opts Options) *Conn {
	if opts.CancelGrace <= 0 {
		opts.CancelGrace = time.Second
	}
	ctx, cancel := context.WithCancel(context.Background())
	c := &Conn{
		rwc: rwc, opts: opts, lim: opts.Limits, ctx: ctx, cancel: cancel,
		changed: make(chan struct{}), pending: map[uint64]*pendingCall{}, handlers: map[uint64]*inflight{},
		recv: map[uint64]*RecvStream{}, send: map[uint64]*SendStream{},
		done: make(chan struct{}), readerDone: make(chan struct{}),
		nextReq: 1, nextStream: 1,
	}
	if opts.Role == RolePlugin {
		c.nextReq, c.nextStream = 2, 2
	}
	go c.readLoop()
	return c
}

// Context is cancelled when the session ends.
func (c *Conn) Context() context.Context { return c.ctx }

// Done is closed when the session has ended.
func (c *Conn) Done() <-chan struct{} { return c.done }

// Err is the reason the session ended, or nil while it is alive.
func (c *Conn) Err() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.err
}

// Limits returns the negotiated limits.
func (c *Conn) Limits() Limits { return c.lim }

// Close ends the session.
func (c *Conn) Close() error {
	c.fail(io.ErrClosedPipe)
	return nil
}

func (c *Conn) fail(err error) {
	c.mu.Lock()
	if c.err != nil {
		c.mu.Unlock()
		return
	}
	c.err = err
	close(c.done)
	c.cancel()
	for _, p := range c.pending {
		if p.timer != nil {
			p.timer.Stop()
		}
	}
	c.broadcastLocked()
	c.mu.Unlock()
	_ = c.rwc.Close()
}

func (c *Conn) broadcastLocked() {
	close(c.changed)
	c.changed = make(chan struct{})
}

func (c *Conn) waitLocked(ctx context.Context) error {
	ch := c.changed
	c.mu.Unlock()
	select {
	case <-ch:
	case <-ctx.Done():
	case <-c.done:
	}
	c.mu.Lock()
	if c.err != nil {
		return c.err
	}
	return ctx.Err()
}

func (c *Conn) write(f Frame) error {
	c.wmu.Lock()
	err := WriteFrame(c.rwc, f)
	c.wmu.Unlock()
	if err != nil {
		c.fail(err)
	}
	return err
}

func (c *Conn) peerParity() uint64 {
	if c.opts.Role == RoleEngine {
		return 0
	}
	return 1
}

func (c *Conn) readLoop() {
	defer close(c.readerDone)
	for {
		f, err := ReadFrame(c.rwc, c.lim)
		if err == nil {
			err = c.dispatch(f)
		}
		if err != nil {
			c.fail(err)
			return
		}
	}
}

func (c *Conn) dispatch(f Frame) error {
	switch f.Type {
	case TypeRequest:
		return c.onRequest(f)
	case TypeResponse:
		return c.onResponse(f)
	case TypeCancel:
		c.mu.Lock()
		if h := c.handlers[f.RequestID]; h != nil {
			h.cancel()
		}
		c.mu.Unlock()
		return nil
	case TypeData:
		return c.onData(f)
	case TypeWindow:
		return c.onWindow(f)
	case TypeEnd:
		return c.onEnd(f)
	default:
		return protoErr("unexpected handshake frame")
	}
}

func (c *Conn) onRequest(f Frame) error {
	var req Request
	if err := Decode(f.Payload, &req); err != nil {
		return err
	}
	if f.RequestID&1 != c.peerParity() {
		return protoErr("request id from wrong side")
	}
	timeout := time.Duration(req.TimeoutMs) * time.Millisecond
	c.mu.Lock()
	if f.RequestID <= c.lastPeer {
		c.mu.Unlock()
		return protoErr("request id reused")
	}
	c.lastPeer = f.RequestID
	if len(c.handlers) >= c.lim.MaxRequests {
		c.mu.Unlock()
		return protoErr("too many in-flight requests")
	}
	depth := 1
	if req.Parent != 0 {
		switch {
		case c.handlers[uint64(req.Parent)] != nil:
			depth = c.handlers[uint64(req.Parent)].depth + 1
		case c.pending[uint64(req.Parent)] != nil && !c.pending[uint64(req.Parent)].cancelled:
			depth = c.pending[uint64(req.Parent)].depth + 1
		case c.resolvedLocked(uint64(req.Parent)):
			// The parent finished while this child was in flight: not a
			// violation, the child is simply refused.
			c.mu.Unlock()
			go c.respond(f.RequestID, nil, NewError(CodeCancelled), nil)
			return nil
		default:
			c.mu.Unlock()
			return protoErr("parent request is not active")
		}
	}
	if depth > MaxNesting {
		c.mu.Unlock()
		return protoErr("requests nested too deeply")
	}
	if timeout < MinTimeoutMs*time.Millisecond || timeout > MaxTimeoutMs*time.Millisecond {
		c.mu.Unlock()
		go c.respond(f.RequestID, nil, NewError(CodeInvalidArgument), nil)
		return nil
	}
	ctx, cancel := context.WithTimeout(c.ctx, timeout)
	c.handlers[f.RequestID] = &inflight{cancel: cancel, depth: depth}
	c.mu.Unlock()
	go c.serve(ctx, cancel, f.RequestID, depth, req)
	return nil
}

func (c *Conn) serve(ctx context.Context, cancel context.CancelFunc, id uint64, depth int, req Request) {
	defer cancel()
	var result any
	var err error
	if c.opts.Handler == nil {
		err = NewError(CodeUnsupported)
	} else {
		func() {
			defer func() {
				if recover() != nil {
					err = NewError(CodePanic)
				}
			}()
			result, err = c.opts.Handler(ctx, &Call{ID: id, Method: req.Method, Parent: uint64(req.Parent), Params: req.Params, Conn: c})
		}()
	}
	c.mu.Lock()
	delete(c.handlers, id)
	c.mu.Unlock()
	c.ReleaseRoot(id)
	c.respond(id, result, err, ctx)
}

// respond sends exactly one response. A request that was cancelled or timed
// out never reports success, so a late result cannot be adopted.
func (c *Conn) respond(id uint64, result any, err error, ctx context.Context) {
	if ctx != nil && ctx.Err() != nil {
		err = ctx.Err()
	}
	var resp Response
	if err != nil {
		resp = Response{Error: toError(err).body()}
	} else {
		if result == nil {
			result = Empty{}
		}
		b, merr := Encode(result)
		if merr != nil || len(b) > c.lim.ControlBytes {
			resp = Response{Error: NewError(CodeInternal).body()}
		} else {
			resp = Response{OK: true, Result: b}
		}
	}
	payload, merr := Encode(resp)
	if merr != nil {
		return
	}
	_ = c.write(Frame{Type: TypeResponse, RequestID: id, Payload: payload})
}

func toError(err error) *Error {
	var e *Error
	switch {
	case errors.As(err, &e):
		return e
	case errors.Is(err, context.DeadlineExceeded):
		return NewError(CodeTimeout)
	case errors.Is(err, context.Canceled):
		return NewError(CodeCancelled)
	}
	return NewError(CodeInternal)
}

func (c *Conn) onResponse(f Frame) error {
	var resp Response
	if err := Decode(f.Payload, &resp); err != nil {
		return err
	}
	if err := resp.validate(); err != nil {
		return err
	}
	if resp.Error != nil && !knownCode(resp.Error.Code) {
		return protoErr("unknown error code")
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	p := c.pending[f.RequestID]
	if p == nil {
		own := f.RequestID&1 != c.peerParity() && f.RequestID < c.nextReq
		if !own {
			return protoErr("response for unknown request")
		}
		return c.drainLocked(len(f.Payload))
	}
	delete(c.pending, f.RequestID)
	if p.timer != nil {
		p.timer.Stop()
	}
	if !p.cancelled {
		p.ch <- resp
	}
	return nil
}

func knownCode(code string) bool {
	for _, k := range knownCodes {
		if k == code {
			return true
		}
	}
	return false
}

func (c *Conn) drainLocked(n int) error {
	c.drained += n
	if c.drained > SessionStreamBudget {
		return protoErr("too many late frames")
	}
	return nil
}

// CallOptions describe an outgoing request.
type CallOptions struct {
	Method  string
	Parent  uint64
	Timeout time.Duration
	Params  any
	// Build, when set, produces Params once the request ID is known so
	// streams can be rooted at it. It must not block on the peer.
	Build func(id uint64) (any, error)
	// Sent, when set, runs once the request frame has been written, in the
	// calling goroutine. Child requests that name this call as Parent may be
	// started from here: they are ordered after the request on the wire. It
	// must not block.
	Sent func(id uint64)
}

// Call sends a request and waits for its response, decoding the result into
// result (nil expects an empty object). It returns the request ID so the
// caller can use streams rooted at it; on success the caller must call
// ReleaseRoot(id) when done with them. On failure they are released here.
func (c *Conn) Call(ctx context.Context, o CallOptions, result any) (uint64, error) {
	timeout := o.Timeout
	if timeout <= 0 || timeout > MaxTimeoutMs*time.Millisecond {
		timeout = MaxTimeoutMs * time.Millisecond
	}
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	if dl, ok := ctx.Deadline(); ok {
		timeout = time.Until(dl)
	}
	if timeout < MinTimeoutMs*time.Millisecond {
		return 0, toError(context.DeadlineExceeded)
	}

	// An ID is allocated and its frame written under reqMu, so concurrent
	// calls reach the wire in ID order.
	c.reqMu.Lock()
	reqHeld := true
	releaseReq := func() {
		if reqHeld {
			reqHeld = false
			c.reqMu.Unlock()
		}
	}
	defer releaseReq()
	c.mu.Lock()
	if c.err != nil {
		c.mu.Unlock()
		return 0, NewError(CodeUnavailable)
	}
	if len(c.pending) >= c.lim.MaxRequests {
		c.mu.Unlock()
		return 0, NewError(CodeExhausted)
	}
	depth := 1
	if o.Parent != 0 {
		switch {
		case c.handlers[o.Parent] != nil:
			depth = c.handlers[o.Parent].depth + 1
		case c.pending[o.Parent] != nil && !c.pending[o.Parent].cancelled:
			depth = c.pending[o.Parent].depth + 1
		default:
			c.mu.Unlock()
			return 0, NewError(CodeInvalidState)
		}
		if depth > MaxNesting {
			c.mu.Unlock()
			return 0, NewError(CodeInvalidArgument)
		}
	}
	id := c.nextReq
	c.nextReq += 2
	pc := &pendingCall{ch: make(chan Response, 1), depth: depth}
	c.pending[id] = pc
	c.mu.Unlock()

	fail := func(err error) (uint64, error) {
		c.mu.Lock()
		delete(c.pending, id)
		c.mu.Unlock()
		c.ReleaseRoot(id)
		return id, err
	}
	params := o.Params
	if o.Build != nil {
		var err error
		if params, err = o.Build(id); err != nil {
			return fail(err)
		}
	}
	rawParams, err := Encode(params)
	if err != nil {
		return fail(NewError(CodeInternal))
	}
	ms := max(int64(MinTimeoutMs), min(int64(time.Until(deadline(ctx))/time.Millisecond), MaxTimeoutMs))
	payload, err := Encode(Request{Method: o.Method, TimeoutMs: U64(ms), Parent: U64(o.Parent), Params: rawParams})
	if err != nil || len(payload) > c.lim.ControlBytes {
		return fail(NewError(CodeLimitExceeded))
	}
	if err := c.write(Frame{Type: TypeRequest, RequestID: id, Payload: payload}); err != nil {
		return fail(NewError(CodeUnavailable))
	}
	releaseReq()
	if o.Sent != nil {
		o.Sent(id)
	}

	select {
	case resp := <-pc.ch:
		if !resp.OK {
			c.ReleaseRoot(id)
			return id, resp.Error.err()
		}
		if result == nil {
			result = &Empty{}
		}
		if err := Decode(resp.Result, result); err != nil {
			c.fail(err)
			return fail(NewError(CodeUnavailable))
		}
		return id, nil
	case <-ctx.Done():
		c.mu.Lock()
		if c.pending[id] == pc {
			pc.cancelled = true
			pc.timer = time.AfterFunc(c.opts.CancelGrace, func() {
				c.mu.Lock()
				stuck := c.pending[id] == pc && c.err == nil
				c.mu.Unlock()
				if stuck && c.opts.OnStuck != nil {
					c.opts.OnStuck()
				}
			})
		}
		c.mu.Unlock()
		_ = c.write(Frame{Type: TypeCancel, RequestID: id})
		c.ReleaseRoot(id)
		return id, toError(ctx.Err())
	case <-c.done:
		c.ReleaseRoot(id)
		return id, NewError(CodeUnavailable)
	}
}

func deadline(ctx context.Context) time.Time {
	dl, _ := ctx.Deadline()
	return dl
}

// resolvedLocked reports whether id names a request that was issued by one
// side and is no longer active. c.mu must be held.
func (c *Conn) resolvedLocked(id uint64) bool {
	if id&1 == c.peerParity() {
		return id <= c.lastPeer
	}
	return id < c.nextReq
}
