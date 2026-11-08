// Package session is the tailcat peer-session layer under the public engine
// package (unic/engine): it keeps a session with every device the user has
// authorized for this backend, proves device identities during the handshake
// (docs/design/identity-protocol.md), and moves clips to and from a Receiver.
// It decides nothing about trust: the Host does. It knows nothing of tailcat
// or of any clipboard; it talks to a Transport, so it is tested with loopback
// TCP. Hosts use unic/engine, not this package.
package session

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/internal/wire"
)

// Transport carries bytes between devices.
type Transport interface {
	// Listen accepts connections from peers.
	Listen(ctx context.Context) (net.Listener, error)
	// Dial connects to the peer with the given address.
	Dial(ctx context.Context, addr string) (net.Conn, error)
	// Addr is this device's own address, which peers dial.
	Addr() string
	// Binding is the identity protocol's context_binding for connections to
	// the device at addr (and, with the device's own Addr, for connections it
	// accepts): both ends compute the same bytes. It also serves as the
	// remote context of the path to that device.
	Binding(addr string) ([]byte, error)
}

type Config struct {
	Identity  *identity.Identity
	Name      string
	Host      Host
	Transport Transport
	Logf      func(format string, args ...any)
	// Settings supplies the user's configuration, re-read on every use so
	// edits apply at once. Nil means the defaults.
	Settings SettingsSource
	// Receiver takes the clips that arrive. Without one they are read and
	// dropped. The session layer never touches a clipboard.
	Receiver Receiver
}

// The types shared with the engine live in peerbackend.
type (
	Standing       = peerbackend.Standing
	Target         = peerbackend.Target
	Verified       = peerbackend.Verified
	Host           = peerbackend.Host
	Receiver       = peerbackend.Receiver
	Admission      = peerbackend.Admission
	Settings       = peerbackend.Settings
	SettingsSource = peerbackend.SettingsSource
	Event          = peerbackend.Event
	TargetState    = peerbackend.TargetState
	Job            = peerbackend.Job
)

const (
	Sending    = peerbackend.Sending
	Sent       = peerbackend.Sent
	Failed     = peerbackend.Failed
	Superseded = peerbackend.Superseded
)

var (
	DefaultSettings  = peerbackend.DefaultSettings
	ErrNotRunning    = peerbackend.ErrNotRunning
	ErrSelf          = peerbackend.ErrSelf
	ErrBadAddress    = peerbackend.ErrBadAddress
	ErrProtocol      = peerbackend.ErrProtocol
	ErrIdentity      = peerbackend.ErrIdentity
	ErrBadProof      = peerbackend.ErrBadProof
	ErrNotAllowed    = peerbackend.ErrNotAllowed
	ErrSessionClosed = peerbackend.ErrSessionClosed
)

type Engine struct {
	cfg Config

	mu       sync.Mutex
	sessions map[string]*session // by peer device id
	subs     map[chan Event]struct{}
	retry    map[string]context.CancelFunc
	ctx      context.Context
	conns    map[net.Conn]struct{} // connections still handshaking, closed on shutdown
	wg       sync.WaitGroup        // every background goroutine of a Run
}

func New(cfg Config) *Engine {
	if cfg.Logf == nil {
		cfg.Logf = log.Printf
	}
	return &Engine{
		cfg:      cfg,
		sessions: map[string]*session{},
		subs:     map[chan Event]struct{}{},
		retry:    map[string]context.CancelFunc{},
		conns:    map[net.Conn]struct{}{},
	}
}

func (e *Engine) logf(f string, a ...any) { e.cfg.Logf(f, a...) }

// settings is the configuration in effect right now.
func (e *Engine) settings() Settings {
	if e.cfg.Settings == nil {
		return DefaultSettings()
	}
	return e.cfg.Settings.Current()
}

// name is this device's name: the configured one, else the default.
func (e *Engine) name() string {
	if n := strings.TrimSpace(e.settings().DeviceName); n != "" {
		return n
	}
	return e.cfg.Name
}

// Run blocks until ctx is done. When it returns every goroutine it started has
// exited, all sessions are closed and Run may be called again (a host that
// suspends networking stops Run and later restarts it).
func (e *Engine) Run(ctx context.Context) error { return e.RunReady(ctx, nil) }

// RunReady is Run that calls ready, if not nil, once the engine is listening
// and accepting work.
func (e *Engine) RunReady(ctx context.Context, ready func()) error {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()

	e.mu.Lock()
	if e.ctx != nil {
		e.mu.Unlock()
		return errors.New("engine is already running")
	}
	e.mu.Unlock()

	ln, err := e.cfg.Transport.Listen(ctx)
	if err != nil {
		return fmt.Errorf("listen: %w", err)
	}
	e.mu.Lock()
	e.ctx = ctx
	e.mu.Unlock()
	defer e.shutdown(ln)

	e.spawn(func() { e.acceptLoop(ctx, ln) })
	e.Reconcile()
	if ready != nil {
		ready()
	}
	<-ctx.Done()
	return ctx.Err()
}

// spawn runs f as a tracked goroutine; it reports false (and does not run f)
// once the engine is shutting down, so that shutdown can wait for everything.
func (e *Engine) spawn(f func()) bool {
	e.mu.Lock()
	if e.ctx == nil || e.ctx.Err() != nil {
		e.mu.Unlock()
		return false
	}
	e.wg.Add(1)
	e.mu.Unlock()
	go func() { defer e.wg.Done(); f() }()
	return true
}

// track registers a connection that is still being set up so that shutdown
// can cut it short. The returned function forgets it again.
func (e *Engine) track(c net.Conn) (untrack func(), ok bool) {
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.ctx == nil || e.ctx.Err() != nil {
		_ = c.Close()
		return func() {}, false
	}
	e.conns[c] = struct{}{}
	return func() { e.mu.Lock(); delete(e.conns, c); e.mu.Unlock() }, true
}

// shutdown stops accepting, drops every connection and waits for the
// goroutines of this Run.
func (e *Engine) shutdown(ln net.Listener) {
	e.mu.Lock()
	e.ctx = nil
	for id, cancel := range e.retry {
		cancel()
		delete(e.retry, id)
	}
	sessions := make([]*session, 0, len(e.sessions))
	for _, s := range e.sessions {
		sessions = append(sessions, s)
	}
	conns := make([]net.Conn, 0, len(e.conns))
	for c := range e.conns {
		conns = append(conns, c)
	}
	e.mu.Unlock()
	_ = ln.Close()
	for _, s := range sessions {
		s.close()
	}
	for _, c := range conns {
		_ = c.Close()
	}
	e.wg.Wait()
}

// Connect makes one connection to the device at addr as the user's own act
// (a pairing ticket, or the path of a device just authorized). expect is the
// DeviceID the address must belong to, "" when not known; tok is the ticket's
// one-time token, nil without a ticket. The device is recorded through the
// Host; a session is made only if the device is already allowed on both sides.
// The returned Standing is valid whenever the identity was proven, even if err
// is ErrNotAllowed or an awaiting error.
func (e *Engine) Connect(ctx context.Context, addr, expect string, tok []byte) (Standing, bool, error) {
	e.mu.Lock()
	started := e.ctx != nil
	e.mu.Unlock()
	if !started {
		return Standing{}, false, ErrNotRunning
	}
	if addr == e.cfg.Transport.Addr() {
		return Standing{}, false, ErrSelf
	}
	conn, err := e.cfg.Transport.Dial(ctx, addr)
	if err != nil {
		return Standing{}, false, fmt.Errorf("connect: %w", err)
	}
	untrack, ok := e.track(conn)
	if !ok {
		return Standing{}, false, ErrNotRunning
	}
	defer untrack()
	s, st, err := e.handshake(conn, true, addr, expect, tok)
	var aw *awaitingError
	switch {
	case err == nil:
	case errors.As(err, &aw):
		_ = conn.Close()
		return st, true, nil
	default:
		_ = conn.Close()
		if errors.Is(err, ErrNotAllowed) {
			return st, false, nil
		}
		return st, false, err
	}
	if e.register(s) {
		if !e.spawn(func() { e.serve(s) }) {
			s.close()
			return st, false, ErrNotRunning
		}
	} else {
		_ = conn.Close() // already connected through a better session
	}
	return st, false, nil
}

// Connected reports whether a session with the device is up.
func (e *Engine) Connected(deviceID string) bool {
	e.mu.Lock()
	defer e.mu.Unlock()
	s := e.sessions[deviceID]
	if s == nil {
		return false
	}
	select {
	case <-s.done:
		return false
	default:
		return true
	}
}

// Self is this device's name and address.
func (e *Engine) Self() (name, addr string) {
	return e.name(), e.cfg.Transport.Addr()
}

// Subscribe returns a channel of events and a function that stops it.
func (e *Engine) Subscribe() (<-chan Event, func()) {
	ch := make(chan Event, 32)
	e.mu.Lock()
	e.subs[ch] = struct{}{}
	e.mu.Unlock()
	return ch, func() {
		e.mu.Lock()
		delete(e.subs, ch)
		e.mu.Unlock()
	}
}

func (e *Engine) emit(ev Event) {
	e.mu.Lock()
	defer e.mu.Unlock()
	for ch := range e.subs {
		select {
		case ch <- ev:
		default: // a slow subscriber misses events instead of stalling the engine
		}
	}
}

// ---- clipboard watching ----

// ---- sessions ----

type outClip struct {
	job *Job
}

func (c outClip) fail(peer string, err error) { c.job.Report(peer, Failed, err) }

// Submit sends a job to every connected device. prepare is called once, before
// anything is queued, with the ids of those devices; it returns the job, or
// false to cancel (for example when there are none). Submit reports whether
// the job was queued.
func (e *Engine) Submit(prepare func(peers []string) (Job, bool)) bool {
	e.mu.Lock()
	ss := make([]*session, 0, len(e.sessions))
	for _, s := range e.sessions {
		select {
		case <-s.done:
		default:
			ss = append(ss, s)
		}
	}
	e.mu.Unlock()
	live := ss[:0]
	for _, s := range ss {
		if st, ok := e.cfg.Host.Standing(s.peerID); ok && st.Allowed && st.SyncEnabled {
			live = append(live, s)
		}
	}
	ss = live
	if len(ss) == 0 {
		return false
	}
	sort.Slice(ss, func(i, j int) bool { return ss[i].peerID < ss[j].peerID })
	ids := make([]string, len(ss))
	for i, s := range ss {
		ids[i] = s.peerID
	}
	j, ok := prepare(ids)
	if !ok {
		return false
	}
	for _, s := range ss {
		s.enqueue(outClip{job: &j})
	}
	return true
}

type session struct {
	e        *Engine
	conn     net.Conn
	w        *wire.Conn
	peerID   string
	peerName string
	peerAddr string
	outgoing bool
	ctl      chan any // control messages, written by sendLoop like everything else
	done     chan struct{}
	once     sync.Once

	omu    sync.Mutex
	pend   *outClip      // the one clip waiting to be sent
	closed bool          // no more clips are accepted
	kick   chan struct{} // a clip is pending
}

// close ends the session. A clip still waiting for its turn fails. close must
// not be called with Engine.mu held: it reports to the job's owner.
func (s *session) close() {
	s.once.Do(func() {
		close(s.done)
		_ = s.conn.Close()
		s.omu.Lock()
		s.closed = true
		p := s.pend
		s.pend = nil
		s.omu.Unlock()
		if p != nil {
			p.fail(s.peerID, ErrSessionClosed)
		}
	})
}

// enqueue keeps only the newest pending clip: a slow link should deliver the
// latest clipboard, not a backlog of stale ones. The clip it replaces, if it
// came from a Job, is reported as superseded.
func (s *session) enqueue(c outClip) {
	s.omu.Lock()
	if s.closed {
		s.omu.Unlock()
		c.fail(s.peerID, ErrSessionClosed)
		return
	}
	old := s.pend
	s.pend = &c
	s.omu.Unlock()
	if old != nil {
		old.job.Report(s.peerID, Superseded, nil)
	}
	select {
	case s.kick <- struct{}{}:
	default:
	}
}

const handshakeTimeout = 15 * time.Second

func (e *Engine) self() string { return e.cfg.Identity.DeviceID() }

// handshake exchanges hellos, then identity proofs, then verdicts. The
// transcript binds both hellos to the transport context, so a proof cannot be
// replayed on another connection or turned around (identity-protocol.md §4).
// expect is the DeviceID the peer must prove ("" accepts any); tok is the
// dialer's ticket token. The returned Standing is set once the proof verified,
// even when the error says the device may not have a session.
func (e *Engine) handshake(conn net.Conn, outgoing bool, dialed, expect string, tok []byte) (*session, Standing, error) {
	_ = conn.SetDeadline(time.Now().Add(handshakeTimeout))
	defer func() { _ = conn.SetDeadline(time.Time{}) }()
	own := e.cfg.Transport.Addr()
	bindAddr := own
	if outgoing {
		bindAddr = dialed
	}
	binding, err := e.cfg.Transport.Binding(bindAddr)
	if err != nil {
		return nil, Standing{}, fmt.Errorf("handshake: %w", err)
	}
	mine, err := identity.NewHello(e.cfg.Identity.Public())
	if err != nil {
		return nil, Standing{}, err
	}
	w := wire.NewConn(conn)
	werr := make(chan error, 1)
	out := wire.Hello{T: "hello", Version: wire.Version, H: mine.Encode(), Name: e.name(), Addr: own}
	go func() { werr <- w.WriteJSON(out) }()
	var h wire.Hello
	if err := readTyped(w, "hello", &h); err != nil {
		return nil, Standing{}, err
	}
	if err := <-werr; err != nil {
		return nil, Standing{}, fmt.Errorf("handshake: %w", err)
	}
	if h.Version != wire.Version {
		return nil, Standing{}, fmt.Errorf("handshake: peer speaks protocol %d, we speak %d: %w", h.Version, wire.Version, ErrProtocol)
	}
	theirs, err := identity.ParseHello(h.H)
	if err != nil {
		return nil, Standing{}, fmt.Errorf("handshake: %w", err)
	}
	if _, err := identity.SelectVersion(mine, theirs); err != nil {
		return nil, Standing{}, fmt.Errorf("handshake: %w: %w", err, ErrProtocol)
	}
	peerID := identity.DeviceID(theirs.PK)
	if peerID == e.self() {
		return nil, Standing{}, ErrSelf
	}
	if expect != "" && peerID != expect {
		return nil, Standing{}, ErrIdentity // before proving ourselves to the wrong device
	}
	helloI, helloR, mineRole, theirRole := mine.Encode(), h.H, identity.RoleInitiator, identity.RoleResponder
	if !outgoing {
		helloI, helloR, mineRole, theirRole = h.H, mine.Encode(), identity.RoleResponder, identity.RoleInitiator
	}
	t, err := identity.Transcript(identity.KindPeerSessions, binding, helloI, helloR)
	if err != nil {
		return nil, Standing{}, fmt.Errorf("handshake: %w", err)
	}
	proof := wire.Proof{T: "proof", Sig: e.cfg.Identity.Prove(mineRole, t)}
	if outgoing && tok != nil {
		proof.Pair = identity.PairProof(tok, t)
	}
	go func() { werr <- w.WriteJSON(proof) }()
	var p wire.Proof
	if err := readTyped(w, "proof", &p); err != nil {
		return nil, Standing{}, err
	}
	if err := <-werr; err != nil {
		return nil, Standing{}, fmt.Errorf("handshake: %w", err)
	}
	if err := identity.VerifyProof(theirs.PK, theirRole, t, p.Sig); err != nil {
		return nil, Standing{}, fmt.Errorf("handshake: %w", ErrBadProof)
	}

	// From here the device is who it says it is, and the host may remember it.
	peerAddr := h.Addr
	if outgoing {
		peerAddr = dialed
	}
	v := Verified{PublicKey: theirs.PK, Name: h.Name, Addr: peerAddr, Ticket: outgoing && len(tok) > 0}
	if !outgoing && len(p.Pair) > 0 {
		v.Ticket = e.cfg.Host.Pair(t, p.Pair)
	}
	if peerAddr != "" {
		v.Context, _ = e.cfg.Transport.Binding(peerAddr) // unusable address: no context, nothing to dial back
	}
	st, err := e.cfg.Host.Verify(v)
	if err != nil {
		return nil, Standing{}, fmt.Errorf("handshake: %w", err)
	}
	go func() { werr <- w.WriteJSON(wire.Verdict{T: "verdict", OK: st.Allowed}) }()
	var vd wire.Verdict
	if err := readTyped(w, "verdict", &vd); err != nil {
		return nil, st, err
	}
	if err := <-werr; err != nil {
		return nil, st, fmt.Errorf("handshake: %w", err)
	}
	s := &session{e: e, conn: conn, w: w, peerID: peerID, peerName: st.Name, peerAddr: peerAddr,
		outgoing: outgoing, kick: make(chan struct{}, 1), ctl: make(chan any, 8), done: make(chan struct{})}
	if !st.Allowed {
		return nil, st, ErrNotAllowed
	}
	if !vd.OK {
		return nil, st, &awaitingError{s}
	}
	return s, st, nil
}

// readTyped reads one control message of the given type into v.
func readTyped(w *wire.Conn, want string, v any) error {
	typ, raw, err := w.ReadRaw()
	if err != nil {
		return fmt.Errorf("handshake: %w", err)
	}
	if typ != want {
		return fmt.Errorf("handshake: expected %q, got %q", want, typ)
	}
	if err := json.Unmarshal(raw, v); err != nil {
		return fmt.Errorf("handshake: %w", err)
	}
	return nil
}

// awaitingError means the other device has not accepted us yet.
type awaitingError struct{ s *session }

func (a *awaitingError) Error() string {
	return a.s.peerName + " has not approved this device yet"
}

func (e *Engine) acceptLoop(ctx context.Context, ln net.Listener) {
	for {
		conn, err := ln.Accept()
		if err != nil {
			if ctx.Err() == nil {
				e.logf("accept: %v", err)
			}
			return
		}
		untrack, ok := e.track(conn)
		if !ok {
			return
		}
		if !e.spawn(func() {
			defer untrack()
			s, _, err := e.handshake(conn, false, "", "", nil)
			if err != nil {
				if !errors.Is(err, ErrNotAllowed) {
					e.logf("incoming connection rejected: %v", err)
				}
				_ = conn.Close()
				return
			}
			if !e.register(s) {
				_ = conn.Close()
				return
			}
			e.serve(s)
		}) {
			untrack()
			_ = conn.Close()
			return
		}
	}
}

// register adds s unless a better session to the same peer exists. Both
// devices apply the same rule, so they agree on which of two simultaneous
// connections survives: the one dialed by the device with the smaller id.
func (e *Engine) register(s *session) bool {
	e.mu.Lock()
	var replaced *session
	if old := e.sessions[s.peerID]; old != nil {
		select {
		case <-old.done:
		default:
			if dialer(e.self(), old) <= dialer(e.self(), s) {
				e.mu.Unlock()
				return false
			}
			replaced = old
		}
	}
	e.sessions[s.peerID] = s
	e.mu.Unlock()
	if replaced != nil {
		replaced.close() // outside the lock: it reports to job owners
	}
	return true
}

func dialer(self string, s *session) string {
	if s.outgoing {
		return self
	}
	return s.peerID
}

// serve runs a registered session until it ends.
func (e *Engine) serve(s *session) {
	e.logf("connected to %s", s.peerName)
	e.cfg.Host.Active(s.peerID, nil)
	e.emit(Event{Kind: "peer-up"})
	if !e.spawn(s.sendLoop) {
		s.close()
	}
	err := s.recvLoop()
	s.close()
	e.mu.Lock()
	if e.sessions[s.peerID] == s {
		delete(e.sessions, s.peerID)
	}
	e.mu.Unlock()
	e.cfg.Host.Active(s.peerID, nil)
	if !errors.Is(err, io.EOF) && !errors.Is(err, net.ErrClosed) {
		e.logf("session with %s ended: %v", s.peerName, err)
	}
	e.emit(Event{Kind: "peer-down"})
}

func (s *session) sendLoop() {
	for {
		select {
		case <-s.done:
			return
		case m := <-s.ctl:
			if err := s.w.WriteJSON(m); err != nil {
				s.e.logf("send to %s: %v", s.peerName, err)
				s.close()
				return
			}
		case <-s.kick:
			s.omu.Lock()
			c := s.pend
			s.pend = nil
			s.omu.Unlock()
			if c != nil && !s.send(c) {
				return
			}
		}
	}
}

// send writes one clip; it reports false when the session is no longer usable.
func (s *session) send(c *outClip) bool {
	c.job.Report(s.peerID, Sending, nil)
	ctx, cancel := context.WithCancel(context.Background())
	go func() {
		select {
		case <-s.done:
			cancel()
		case <-ctx.Done():
		}
	}()
	broken, err := c.job.Send(ctx, s.w, s.e.self())
	cancel()
	if err != nil {
		select {
		case <-s.done: // the connection ended, whatever the error looked like
			err = ErrSessionClosed
		default:
		}
		c.job.Report(s.peerID, Failed, err)
		if broken {
			s.close() // the stream may be out of step: start over
			return false
		}
		return true
	}
	c.job.Report(s.peerID, Sent, nil)
	return true
}

func (s *session) recvLoop() error {
	for {
		typ, raw, err := s.w.ReadRaw()
		if err != nil {
			return err
		}
		switch typ {
		case "clip":
			var m wire.Clip
			if err := json.Unmarshal(raw, &m); err != nil {
				return err
			}
			if err := m.Validate(); err != nil {
				return err
			}
			// A clip this device does not want (paused, or over its own
			// limits) is read and dropped, so the stream stays in sync and
			// the connection survives.
			cfg := s.e.settings()
			if st, ok := s.e.cfg.Host.Standing(s.peerID); !ok || !st.Allowed || !st.SyncEnabled {
				cfg.SyncEnabled = false // the path is paused or revoked
			}
			lim := wire.Limits{File: cfg.MaxFileBytes, Total: cfg.MaxTotalBytes}
			var adm Admission
			wanted := false
			if s.e.cfg.Receiver != nil {
				adm, wanted = s.e.cfg.Receiver.Admit(s.peerID, m)
			}
			if lerr := m.Fits(lim); !cfg.SyncEnabled || lerr != nil || !wanted {
				if wanted {
					adm.Done(clip.Snapshot{}, "", false) // give the reservation back
				}
				if lerr != nil {
					s.e.logf("dropped clip from %s: %v", s.peerName, lerr)
				}
				if err := s.w.Discard(&m); err != nil {
					return err
				}
				continue
			}
			snap, dir, err := s.w.ReceiveClip(&m, adm.Inbox)
			if err != nil {
				adm.Done(clip.Snapshot{}, "", false)
				return err
			}
			if snap.Hash() != m.Hash {
				// Corrupt or tampered: the data is already consumed, so the
				// stream is still in sync; just drop it.
				if dir != "" {
					_ = os.RemoveAll(dir)
				}
				adm.Done(clip.Snapshot{}, "", false)
				s.e.logf("dropped clip from %s: hash mismatch", s.peerName)
				continue
			}
			adm.Done(snap, dir, true)
		default:
			// Unknown control messages are ignored for forward compatibility.
		}
	}
}

// keepDialing maintains an outgoing connection to t, with backoff.
func (e *Engine) keepDialing(t Target) {
	e.mu.Lock()
	if e.retry[t.DeviceID] != nil || e.ctx == nil {
		e.mu.Unlock()
		return
	}
	ctx, cancel := context.WithCancel(e.ctx)
	e.retry[t.DeviceID] = cancel
	e.mu.Unlock()

	if !e.spawn(func() {
		backoff := time.Second
		for ctx.Err() == nil {
			e.mu.Lock()
			up := e.sessions[t.DeviceID] != nil
			e.mu.Unlock()
			if up {
				if !sleep(ctx, 5*time.Second) {
					return
				}
				backoff = time.Second
				continue
			}
			dctx, dcancel := context.WithTimeout(ctx, 30*time.Second)
			conn, err := e.cfg.Transport.Dial(dctx, t.Addr)
			dcancel()
			var s *session
			if err == nil {
				untrack, ok := e.track(conn)
				if !ok {
					return
				}
				s, _, err = e.handshake(conn, true, t.Addr, t.DeviceID, nil)
				untrack()
				if err != nil {
					_ = conn.Close()
				}
			}
			if err != nil {
				e.cfg.Host.Active(t.DeviceID, err)
				if ctx.Err() == nil {
					e.logf("cannot reach %s: %v (retry in %v)", t.Name, err, backoff)
				}
				if !sleep(ctx, backoff) {
					return
				}
				if backoff < time.Minute {
					backoff *= 2
				}
				continue
			}
			backoff = time.Second
			// The user may have revoked the device while we were connecting.
			if st, ok := e.cfg.Host.Standing(t.DeviceID); !ok || !st.Allowed {
				_ = conn.Close()
				return
			}
			if !e.register(s) {
				_ = conn.Close()
				continue
			}
			e.serve(s)
		}
	}) {
		cancel()
		e.mu.Lock()
		delete(e.retry, t.DeviceID)
		e.mu.Unlock()
	}
}

func sleep(ctx context.Context, d time.Duration) bool {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-t.C:
		return true
	}
}

// Reconcile makes the connections match the host's authorizations: devices
// the user has revoked or removed lose their session and retries at once, and
// every authorized device is dialed. The host calls it after every change of
// trust, binding or endpoint.
func (e *Engine) Reconcile() {
	targets := e.cfg.Host.Targets()
	want := make(map[string]Target, len(targets))
	for _, t := range targets {
		want[t.DeviceID] = t
	}
	var drop []*session
	e.mu.Lock()
	running := e.ctx != nil
	for id, cancel := range e.retry {
		if t, ok := want[id]; !ok {
			cancel()
			delete(e.retry, id)
		} else if e.sessions[id] == nil && t.Addr == "" {
			cancel()
			delete(e.retry, id)
		}
	}
	for id, s := range e.sessions {
		if _, ok := want[id]; !ok {
			drop = append(drop, s)
		}
	}
	e.mu.Unlock()
	for _, s := range drop {
		s.close() // outside the lock: it reports to job owners
	}
	if !running {
		return
	}
	for _, t := range targets {
		if t.Addr != "" {
			e.keepDialing(t)
		}
	}
}

// NetworkChanged tells the engine the network may have changed: devices that
// are not connected are retried at once instead of after their backoff. It
// does not resend anything.
func (e *Engine) NetworkChanged() {
	for _, t := range e.cfg.Host.Targets() {
		e.mu.Lock()
		up := e.sessions[t.DeviceID] != nil
		cancel := e.retry[t.DeviceID]
		if !up && cancel != nil {
			delete(e.retry, t.DeviceID)
		}
		e.mu.Unlock()
		if !up && cancel != nil {
			cancel()
			e.keepDialing(t)
		}
	}
}
