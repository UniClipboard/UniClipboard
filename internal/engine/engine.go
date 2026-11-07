// Package engine is the session layer under the public engine package
// (unic/engine): it keeps a session with every paired device, handles pairing
// and introductions, and moves clips to and from a Receiver. It knows nothing
// of tailcat or of any clipboard; it talks to a Transport, so it is tested with
// loopback TCP. Hosts use unic/engine, not this package.
package engine

import (
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
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

	"unic/internal/clip"
	"unic/internal/rendezvous"
	"unic/internal/store"
	"unic/internal/wire"
)

// Transport carries bytes between devices.
type Transport interface {
	// Listen accepts connections from peers.
	Listen(ctx context.Context) (net.Listener, error)
	// Dial connects to the peer with the given address.
	Dial(ctx context.Context, addr string) (net.Conn, error)
	// Addr is this device's own address, which peers dial.
	Addr() string
}

type Config struct {
	ID        string // stable device id
	Name      string
	Store     *store.Store
	Transport Transport
	Logf      func(format string, args ...any)
	// Rendezvous stores pairing codes; nil disables them (full addresses
	// still work).
	Rendezvous *rendezvous.Client
	// Settings supplies the user's configuration, re-read on every use so
	// edits apply at once. Nil means the defaults.
	Settings SettingsSource
	// Receiver takes the clips that arrive. Without one they are read and
	// dropped. The session layer never touches a clipboard.
	Receiver Receiver
}

// Receiver stores clips that arrive from other devices.
type Receiver interface {
	// Admit is asked before any byte of a clip is read. It returns false to
	// have the clip read and dropped (no room, say). Otherwise the clip is
	// read into Inbox and handed to Done, which must be called exactly once:
	// with ok=true and the clip (dir is the directory holding its files, owned
	// by the receiver from then on), or with ok=false if it was not delivered.
	Admit(peerID string, m wire.Clip) (Admission, bool)
}

// Admission is the receiver's acceptance of one clip.
type Admission struct {
	Inbox string
	Done  func(snap clip.Snapshot, dir string, ok bool)
}

// Settings are the parts of the user's configuration the session layer acts on.
type Settings struct {
	DeviceName    string
	RendezvousURL string
	MaxFileBytes  int64
	MaxTotalBytes int64
	SyncEnabled   bool
}

// DefaultSettings: sync on, the protocol's own limits.
func DefaultSettings() Settings {
	return Settings{MaxFileBytes: wire.MaxFileBytes, MaxTotalBytes: wire.MaxTotal, SyncEnabled: true}
}

// SettingsSource provides the settings in effect.
type SettingsSource interface{ Current() Settings }

// Sentinel errors, so that callers can classify failures without matching text.
var (
	ErrNotRunning = errors.New("engine is not running")
	ErrSelf       = errors.New("that is this device itself")
	ErrProtocol   = errors.New("peer speaks another protocol version")
	ErrIdentity   = errors.New("address belongs to a different device")
	ErrNoPending  = errors.New("no such pending device")
)

type Engine struct {
	cfg Config

	mu       sync.Mutex
	sessions map[string]*session // by peer id
	subs     map[chan Event]struct{}
	retry    map[string]context.CancelFunc
	pairing  map[string]int // addresses the user is pairing with right now
	code     *liveCode      // the pairing code this device is showing, if any
	ctx      context.Context
	rvCache  *rendezvous.Client    // client for the configured rendezvous_url
	conns    map[net.Conn]struct{} // connections still handshaking, closed on shutdown
	wg       sync.WaitGroup        // every background goroutine of a Run
}

// liveCode is a pairing code registered for this device.
type liveCode struct {
	rendezvous.Code
	token string
}

// Event tells that something changed: "peer-up", "peer-down", "peers",
// "pending" or "code". It carries nothing else; whoever listens looks.
type Event struct {
	Kind string
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
		pairing:  map[string]int{},
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

// rv is the pairing code client, following rendezvous_url. Nil when codes
// are disabled.
func (e *Engine) rv() *rendezvous.Client {
	u := strings.TrimRight(e.settings().RendezvousURL, "/")
	if u == "" || e.cfg.Rendezvous == nil {
		return e.cfg.Rendezvous
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.rvCache == nil || e.rvCache.URL != u {
		e.rvCache = rendezvous.NewWithURL(u)
	}
	return e.rvCache
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
	peers, err := e.cfg.Store.Peers()
	if err != nil {
		return err
	}
	for _, p := range peers {
		e.keepDialing(p)
	}
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

// AddPeer pairs with the device at addr. It connects once synchronously so
// that a wrong address is reported to the user; the connection is then kept
// alive in the background. awaiting is true when the other device has not
// approved this one yet: it is remembered and connects as soon as it does.
func (e *Engine) AddPeer(ctx context.Context, addr string) (p store.Peer, awaiting bool, err error) {
	return e.addPeer(ctx, addr, "")
}

// addPeer pairs with addr; pair is the token of the code that led there.
func (e *Engine) addPeer(ctx context.Context, addr, pair string) (p store.Peer, awaiting bool, err error) {
	e.mu.Lock()
	started := e.ctx != nil
	if started {
		e.pairing[addr]++
	}
	e.mu.Unlock()
	if !started {
		return p, false, ErrNotRunning
	}
	defer func() {
		e.mu.Lock()
		if e.pairing[addr]--; e.pairing[addr] <= 0 {
			delete(e.pairing, addr)
		}
		e.mu.Unlock()
	}()
	if addr == e.cfg.Transport.Addr() {
		return p, false, ErrSelf
	}
	conn, err := e.cfg.Transport.Dial(ctx, addr)
	if err != nil {
		return p, false, fmt.Errorf("connect: %w", err)
	}
	untrack, ok := e.track(conn)
	if !ok {
		return p, false, ErrNotRunning
	}
	defer untrack()
	s, err := e.handshake(conn, true, addr, pair)
	if err != nil {
		var aw *awaitingError
		if !errors.As(err, &aw) {
			_ = conn.Close()
			return p, false, err
		}
		s, awaiting = aw.s, true
	}
	p = store.Peer{ID: s.peerID, Name: s.peerName, Addr: addr}
	if err := e.cfg.Store.UpsertPeer(p); err != nil {
		_ = conn.Close()
		return p, false, err
	}
	_ = e.cfg.Store.DeleteCandidate(p.ID) // the user's own pairing outranks a pending request
	e.keepDialing(p)                      // reconnect after failures; a no-op while a session is up
	e.emit(Event{Kind: "peers"})
	if awaiting {
		_ = conn.Close()
		return p, true, nil
	}
	if e.register(s) {
		if !e.spawn(func() { e.serve(s) }) {
			s.close()
			return p, false, ErrNotRunning
		}
	} else {
		_ = conn.Close() // already connected through a better session
	}
	return p, false, nil
}

func (e *Engine) RemovePeer(id string) (bool, error) {
	ok, err := e.cfg.Store.DeletePeer(id)
	if err != nil {
		return false, err
	}
	e.mu.Lock()
	if cancel := e.retry[id]; cancel != nil {
		cancel()
		delete(e.retry, id)
	}
	s := e.sessions[id]
	e.mu.Unlock()
	if s != nil {
		s.close()
	}
	e.emit(Event{Kind: "peers"})
	return ok, nil
}

// PeerStatus is a paired device and whether it is connected right now.
type PeerStatus struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Connected bool   `json:"connected"`
	LastSeen  int64  `json:"last_seen,omitempty"`
	AddedAt   int64  `json:"-"` // unix seconds; not part of the HTTP API
}

func (e *Engine) Peers() ([]PeerStatus, error) {
	ps, err := e.cfg.Store.Peers()
	if err != nil {
		return nil, err
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	out := make([]PeerStatus, 0, len(ps))
	for _, p := range ps {
		st := PeerStatus{ID: p.ID, Name: p.Name, Connected: e.sessions[p.ID] != nil, AddedAt: p.AddedAt.Unix()}
		if !p.LastSeen.IsZero() {
			st.LastSeen = p.LastSeen.Unix()
		}
		out = append(out, st)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
	return out, nil
}

func (e *Engine) Self() (id, name, addr string) {
	return e.cfg.ID, e.name(), e.cfg.Transport.Addr()
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

// TargetState is how far a clip got towards one device.
type TargetState string

const (
	Sending    TargetState = "sending"
	Sent       TargetState = "sent"
	Failed     TargetState = "failed"
	Superseded TargetState = "superseded"
)

// ErrSessionClosed is the failure of a clip whose connection ended before or
// while it was sent.
var ErrSessionClosed = errors.New("connection closed")

// Job is a clip that a host submits to every connected device. Report and Send
// are called from the session's goroutines, never with an engine lock held.
type Job struct {
	// Report tells how the clip fares towards peer: Sending, then Sent or
	// Failed; or Superseded if a newer clip replaced it before it started.
	Report func(peer string, st TargetState, err error)
	// Send writes the clip to w; origin is this device's id. ctx ends when the
	// connection does. It reports broken=true when it may have written part of
	// a message, in which case the connection is dropped.
	Send func(ctx context.Context, w *wire.Conn, origin string) (broken bool, err error)
}

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

// awaitingError means the other device has not accepted us yet.
type awaitingError struct{ s *session }

func (a *awaitingError) Error() string {
	return a.s.peerName + " has not approved this device yet"
}

// errNotTrusted means we did not accept the other device: it is now a
// candidate waiting for the user.
var errNotTrusted = errors.New("device is not paired; waiting for the user to approve it")

// trusts reports whether we accept h without asking: a paired device, or one
// whose address the user is pairing with at this very moment.
func (e *Engine) trusts(h wire.Hello) bool {
	if ok, err := e.cfg.Store.HasPeer(h.ID); err == nil && ok {
		return true
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	return h.Addr != "" && e.pairing[h.Addr] > 0
}

// handshake exchanges hellos, then verdicts. Dialing a device is itself the
// decision to trust it; accepting a connection needs a paired device.
func (e *Engine) handshake(conn net.Conn, outgoing bool, dialed, pair string) (*session, error) {
	_ = conn.SetDeadline(time.Now().Add(handshakeTimeout))
	defer func() { _ = conn.SetDeadline(time.Time{}) }()
	w := wire.NewConn(conn)
	hello := wire.Hello{T: "hello", Version: wire.Version, ID: e.cfg.ID, Name: e.name(), Addr: e.cfg.Transport.Addr(), Pair: pair}
	werr := make(chan error, 1)
	go func() { werr <- w.WriteJSON(hello) }()
	typ, raw, err := w.ReadRaw()
	if err != nil {
		return nil, fmt.Errorf("handshake: %w", err)
	}
	if err := <-werr; err != nil {
		return nil, fmt.Errorf("handshake: %w", err)
	}
	if typ != "hello" {
		return nil, fmt.Errorf("handshake: unexpected %q message", typ)
	}
	var h wire.Hello
	if err := json.Unmarshal(raw, &h); err != nil {
		return nil, err
	}
	if h.ID == "" || h.ID == e.cfg.ID {
		return nil, errors.New("handshake: bad device id")
	}
	if h.Version != wire.Version {
		return nil, fmt.Errorf("handshake: peer speaks protocol %d, we speak %d: %w", h.Version, wire.Version, ErrProtocol)
	}

	trust := outgoing || e.trusts(h)
	go func() { werr <- w.WriteJSON(wire.Verdict{T: "verdict", OK: trust}) }()
	typ, raw, err = w.ReadRaw()
	if err != nil {
		return nil, fmt.Errorf("handshake: %w", err)
	}
	if err := <-werr; err != nil {
		return nil, fmt.Errorf("handshake: %w", err)
	}
	var v wire.Verdict
	if typ != "verdict" || json.Unmarshal(raw, &v) != nil {
		return nil, fmt.Errorf("handshake: expected a verdict, got %q", typ)
	}

	s := &session{e: e, conn: conn, w: w, peerID: h.ID, peerName: h.Name, peerAddr: h.Addr,
		outgoing: outgoing, kick: make(chan struct{}, 1), ctl: make(chan any, 8), done: make(chan struct{})}
	if outgoing && dialed != "" && s.peerAddr == "" {
		s.peerAddr = dialed
	}
	if !trust {
		if h.Addr != "" { // without an address we could never connect back
			e.addCandidate(store.Candidate{ID: h.ID, Name: h.Name, Addr: h.Addr, ByCode: e.useCode(h.Pair)}, true)
		}
		return nil, errNotTrusted
	}
	if !v.OK {
		return nil, &awaitingError{s}
	}
	return s, nil
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
			s, err := e.handshake(conn, false, "", "")
			if err != nil {
				if !errors.Is(err, errNotTrusted) {
					e.logf("incoming connection rejected: %v", err)
				}
				_ = conn.Close()
				return
			}
			// Trusted: remember the name and the current address, so that we
			// can reconnect after a restart.
			if s.peerAddr != "" {
				if err := e.cfg.Store.UpsertPeer(store.Peer{ID: s.peerID, Name: s.peerName, Addr: s.peerAddr}); err != nil {
					e.logf("remember peer: %v", err)
				}
				_ = e.cfg.Store.DeleteCandidate(s.peerID)
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
			if dialer(e.cfg.ID, old) <= dialer(e.cfg.ID, s) {
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
	_ = e.cfg.Store.TouchPeer(s.peerID)
	e.emit(Event{Kind: "peer-up"})
	if !e.spawn(s.sendLoop) {
		s.close()
	}
	e.introduceTo(s)
	err := s.recvLoop()
	s.close()
	e.mu.Lock()
	if e.sessions[s.peerID] == s {
		delete(e.sessions, s.peerID)
	}
	e.mu.Unlock()
	_ = e.cfg.Store.TouchPeer(s.peerID)
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
	broken, err := c.job.Send(ctx, s.w, s.e.cfg.ID)
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
		case "peers":
			var m wire.Peers
			if err := json.Unmarshal(raw, &m); err != nil {
				return err
			}
			s.e.introduced(s, m)
		default:
			// Unknown control messages are ignored for forward compatibility.
		}
	}
}

// keepDialing maintains an outgoing connection to p, with backoff.
func (e *Engine) keepDialing(p store.Peer) {
	e.mu.Lock()
	if e.retry[p.ID] != nil || e.ctx == nil {
		e.mu.Unlock()
		return
	}
	ctx, cancel := context.WithCancel(e.ctx)
	e.retry[p.ID] = cancel
	e.mu.Unlock()

	if !e.spawn(func() {
		backoff := time.Second
		for ctx.Err() == nil {
			e.mu.Lock()
			up := e.sessions[p.ID] != nil
			e.mu.Unlock()
			if up {
				if !sleep(ctx, 5*time.Second) {
					return
				}
				backoff = time.Second
				continue
			}
			dctx, dcancel := context.WithTimeout(ctx, 30*time.Second)
			conn, err := e.cfg.Transport.Dial(dctx, p.Addr)
			dcancel()
			var s *session
			if err == nil {
				untrack, ok := e.track(conn)
				if !ok {
					return
				}
				s, err = e.handshake(conn, true, p.Addr, "")
				untrack()
				if err != nil {
					_ = conn.Close()
				}
			}
			if err == nil && s.peerID != p.ID {
				_ = conn.Close()
				err = ErrIdentity
			}
			if err != nil {
				var aw *awaitingError
				if errors.As(err, &aw) {
					_ = conn.Close()
				}
				if ctx.Err() == nil {
					e.logf("cannot reach %s: %v (retry in %v)", p.Name, err, backoff)
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
			// The user may have removed the device while we were connecting.
			if ok, err := e.cfg.Store.HasPeer(p.ID); err == nil && !ok {
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
		delete(e.retry, p.ID)
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

// ---- introductions and approval ----

// PendingPeer is a device waiting for the user's approval.
type PendingPeer struct {
	ID   string `json:"id"`
	Name string `json:"name"`
	Via  string `json:"via,omitempty"` // who introduced it; empty if it connected to us directly
	// ByCode is true when it connected with the pairing code this device showed.
	ByCode bool `json:"by_code,omitempty"`
	// Fingerprint is shown on both devices so the user can check that the
	// request comes from the device they think it does.
	Fingerprint string `json:"fingerprint"`
	Time        int64  `json:"time"`
}

func (e *Engine) addCandidate(c store.Candidate, direct bool) {
	created, err := e.cfg.Store.AddCandidate(c, direct)
	if err != nil {
		e.logf("remember candidate: %v", err)
		return
	}
	if created {
		e.emit(Event{Kind: "pending"})
	}
}

// Pending lists the devices waiting for approval.
func (e *Engine) Pending() ([]PendingPeer, error) {
	cs, err := e.cfg.Store.Candidates()
	if err != nil {
		return nil, err
	}
	out := make([]PendingPeer, 0, len(cs))
	for _, c := range cs {
		out = append(out, PendingPeer{ID: c.ID, Name: c.Name, Via: c.Via, ByCode: c.ByCode,
			Fingerprint: e.Fingerprint(c.ID), Time: c.Seen.Unix()})
	}
	return out, nil
}

// Approve pairs with a pending device and connects to it. Its own device
// still has to approve this one, unless it paired with us first.
func (e *Engine) Approve(id string) (store.Peer, error) {
	c, ok, err := e.cfg.Store.Candidate(id)
	if err != nil {
		return store.Peer{}, err
	}
	if !ok {
		return store.Peer{}, ErrNoPending
	}
	p := store.Peer{ID: c.ID, Name: c.Name, Addr: c.Addr}
	if err := e.cfg.Store.UpsertPeer(p); err != nil {
		return p, err
	}
	_ = e.cfg.Store.DeleteCandidate(id)
	e.keepDialing(p)
	e.introduceToAll() // let the others know about the new device
	e.emit(Event{Kind: "peers"})
	return p, nil
}

// Reject dismisses a pending device. Introductions do not bring it back; a
// connection attempt from the device itself does.
func (e *Engine) Reject(id string) error {
	ok, err := e.cfg.Store.RejectCandidate(id)
	if err != nil {
		return err
	}
	if !ok {
		return ErrNoPending
	}
	e.emit(Event{Kind: "peers"})
	return nil
}

// introduceTo tells s's device about our other paired devices.
func (e *Engine) introduceTo(s *session) {
	ps, err := e.cfg.Store.Peers()
	if err != nil {
		return
	}
	msg := wire.Peers{T: "peers"}
	for _, p := range ps {
		if p.ID != s.peerID && p.Addr != "" {
			msg.Peers = append(msg.Peers, wire.PeerInfo{ID: p.ID, Name: p.Name, Addr: p.Addr})
		}
	}
	if len(msg.Peers) == 0 {
		return
	}
	select {
	case s.ctl <- msg:
	default:
	}
}

func (e *Engine) introduceToAll() {
	e.mu.Lock()
	ss := make([]*session, 0, len(e.sessions))
	for _, s := range e.sessions {
		ss = append(ss, s)
	}
	e.mu.Unlock()
	for _, s := range ss {
		e.introduceTo(s)
	}
}

// introduced handles a peer's introductions: unknown devices become
// candidates. Nothing connects until the user approves.
func (e *Engine) introduced(from *session, m wire.Peers) {
	if len(m.Peers) > wire.MaxIntroduced {
		m.Peers = m.Peers[:wire.MaxIntroduced]
	}
	for _, pi := range m.Peers {
		if pi.ID == "" || pi.ID == e.cfg.ID || pi.Addr == "" || pi.Addr == e.cfg.Transport.Addr() {
			continue
		}
		if ok, err := e.cfg.Store.HasPeer(pi.ID); err != nil || ok {
			continue
		}
		e.addCandidate(store.Candidate{ID: pi.ID, Name: pi.Name, Addr: pi.Addr, Via: from.peerName}, false)
	}
}

// ---- pairing codes ----

// ErrNoCodes means this daemon has no rendezvous client.
var ErrNoCodes = &rendezvous.Error{Reason: rendezvous.Unavailable}

// IssueCode registers a fresh pairing code for this device, replacing the one
// it showed before. Another device that types it in finds this one, and
// appears here as a pending device; the code itself grants no trust.
func (e *Engine) IssueCode(ctx context.Context) (rendezvous.Code, error) {
	rv := e.rv()
	addr := e.cfg.Transport.Addr()
	if rv == nil {
		return rendezvous.Code{}, ErrNoCodes
	}
	if addr == "" {
		return rendezvous.Code{}, ErrNotRunning
	}
	token := rendezvous.NewToken()
	c, err := rv.Create(ctx, rendezvous.Ticket{Addr: addr, Token: token})
	if err != nil {
		return c, err
	}
	e.mu.Lock()
	old := e.code
	e.code = &liveCode{Code: c, token: token}
	e.mu.Unlock()
	if old != nil {
		e.spawn(func() { e.consume(old.Code.Code) })
	}
	return c, nil
}

// CurrentCode returns the code this device is showing, if it is still valid.
func (e *Engine) CurrentCode() (rendezvous.Code, bool) {
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.code == nil || !time.Now().Before(e.code.Expires) {
		return rendezvous.Code{}, false
	}
	return e.code.Code, true
}

// CancelCode withdraws the code this device is showing.
func (e *Engine) CancelCode() {
	e.mu.Lock()
	old := e.code
	e.code = nil
	e.mu.Unlock()
	if old != nil {
		e.spawn(func() { e.consume(old.Code.Code) })
	}
}

// useCode reports whether token belongs to the live code, and if so uses the
// code up: one code, one device.
func (e *Engine) useCode(token string) bool {
	if token == "" {
		return false
	}
	e.mu.Lock()
	c := e.code
	ok := c != nil && time.Now().Before(c.Expires) &&
		subtle.ConstantTimeCompare([]byte(token), []byte(c.token)) == 1
	if ok {
		e.code = nil
	}
	e.mu.Unlock()
	if ok {
		e.spawn(func() { e.consume(c.Code.Code) })
		e.emit(Event{Kind: "code"})
	}
	return ok
}

func (e *Engine) consume(code string) {
	rv := e.rv()
	if rv == nil {
		return
	}
	e.mu.Lock()
	parent := e.ctx
	e.mu.Unlock()
	if parent == nil {
		return
	}
	ctx, cancel := context.WithTimeout(parent, 10*time.Second)
	defer cancel()
	rv.Consume(ctx, code)
}

// Join pairs with the device showing code. Like AddPeer, the other device
// still has to approve this one.
func (e *Engine) Join(ctx context.Context, code string) (p store.Peer, awaiting bool, err error) {
	return e.JoinWith(ctx, code, nil)
}

// JoinWith is Join that calls connecting, if not nil, once the code has been
// resolved and the connection is being made.
func (e *Engine) JoinWith(ctx context.Context, code string, connecting func()) (p store.Peer, awaiting bool, err error) {
	rv := e.rv()
	if rv == nil {
		return p, false, ErrNoCodes
	}
	t, err := rv.Resolve(ctx, code)
	if err != nil {
		return p, false, err
	}
	if t.Addr == e.cfg.Transport.Addr() {
		return p, false, ErrSelf
	}
	if connecting != nil {
		connecting()
	}
	p, awaiting, err = e.addPeer(ctx, t.Addr, t.Token)
	if err == nil {
		e.spawn(func() { e.consume(code) })
	}
	return p, awaiting, err
}

// Fingerprint is a short value both this device and peer compute alike, for
// the user to compare before approving: 4 groups of 4 hex digits.
func (e *Engine) Fingerprint(peer string) string { return Fingerprint(e.cfg.ID, peer) }

func Fingerprint(a, b string) string {
	if b < a {
		a, b = b, a
	}
	sum := sha256.Sum256([]byte("unic-pair-fp\x00" + a + "\x00" + b))
	h := strings.ToUpper(hex.EncodeToString(sum[:8]))
	return h[0:4] + " " + h[4:8] + " " + h[8:12] + " " + h[12:16]
}

// NetworkChanged tells the engine the network may have changed: devices that
// are not connected are retried at once instead of after their backoff. It
// does not resend anything.
func (e *Engine) NetworkChanged() {
	ps, err := e.cfg.Store.Peers()
	if err != nil {
		return
	}
	for _, p := range ps {
		e.mu.Lock()
		up := e.sessions[p.ID] != nil
		cancel := e.retry[p.ID]
		if !up && cancel != nil {
			delete(e.retry, p.ID)
		}
		e.mu.Unlock()
		if !up && cancel != nil {
			cancel()
			e.keepDialing(p)
		}
	}
}
