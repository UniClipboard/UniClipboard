// Package session is the peer-session layer of the built-in tailcat backend.
// It keeps a connection to every device the Engine lists in its state
// snapshot, carries the identity handshake (docs/design/identity-protocol.md)
// between the two ends and the Engine's authority, and moves clips to and
// from the Engine. It decides nothing about trust: the Engine does, through
// plugin.PeerAuthority. It knows nothing of tailcat or of any clipboard; it
// talks to a Transport, so it is tested with loopback TCP.
//
// Backend implements plugin.PeerBackend, PeerPairing and PeerDispatcher, the
// same interfaces an out-of-process peer_sessions backend serves over RPC.
package session

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/wire"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Transport carries bytes between devices.
type Transport interface {
	// Start brings the network up (once); Close takes it down for good.
	Start() error
	Close() error
	// Listen accepts connections from peers. It may be called again after the
	// previous listener was closed.
	Listen(ctx context.Context) (net.Listener, error)
	// Dial connects to the peer with the given address.
	Dial(ctx context.Context, addr string) (net.Conn, error)
	// Addr is this device's own address, which peers dial (a secret).
	Addr() string
	// Binding is the identity protocol's context_binding for connections to
	// the device at addr (and, with the device's own Addr, for connections it
	// accepts): both ends compute the same bytes.
	Binding(addr string) ([]byte, error)
}

type Config struct {
	Transport Transport
	Logf      func(format string, args ...any)
}

var (
	// ErrProtocol: the peer speaks another wire version.
	ErrProtocol = errors.New("peer speaks another protocol version")
	// ErrBadAddress: a peer address the transport cannot use.
	ErrBadAddress = errors.New("invalid peer address")
	b64           = base64.RawURLEncoding
)

const (
	handshakeTimeout = 15 * time.Second
	eventTimeout     = 5 * time.Second
)

// Backend is the session layer. The Engine binds it, then drives it with
// state snapshots: the network is up exactly while the latest state is active.
type Backend struct {
	cfg  Config
	auth plugin.PeerAuthority
	recv plugin.PeerReceiver

	life sync.Mutex // serializes bringing the network up and down

	mu       sync.Mutex
	bound    bool
	self     string
	st       rpc.SyncState
	trTried  bool          // Transport.Start was ever called; Close must release it
	trUsed   bool          // Transport.Start was called
	trDone   chan struct{} // closed when that call returned
	run      *runState     // non-nil while the network is up
	sessions map[string]*session
	dialers  map[string]*dialer
	gen      uint64
}

// runState is one up period of the network.
type runState struct {
	ctx    context.Context
	cancel context.CancelFunc
	ln     net.Listener
	conns  map[net.Conn]struct{} // connections still handshaking
	wg     sync.WaitGroup
}

func New(cfg Config) *Backend {
	if cfg.Logf == nil {
		cfg.Logf = func(string, ...any) {} // session logs name devices; never default to the global logger
	}
	return &Backend{cfg: cfg, sessions: map[string]*session{}, dialers: map[string]*dialer{}}
}

func (b *Backend) logf(f string, a ...any) { b.cfg.Logf(f, a...) }

func (b *Backend) SetPeerAuthority(a plugin.PeerAuthority) { b.mu.Lock(); b.auth = a; b.mu.Unlock() }
func (b *Backend) SetPeerReceiver(r plugin.PeerReceiver)   { b.mu.Lock(); b.recv = r; b.mu.Unlock() }

func (b *Backend) authority() plugin.PeerAuthority { b.mu.Lock(); defer b.mu.Unlock(); return b.auth }
func (b *Backend) receiver() plugin.PeerReceiver   { b.mu.Lock(); defer b.mu.Unlock(); return b.recv }
func (b *Backend) selfID() string                  { b.mu.Lock(); defer b.mu.Unlock(); return b.self }

// Bind records the one-time configuration. It never connects.
func (b *Backend) Bind(_ context.Context, bd rpc.Bind) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.bound {
		return rpc.NewError(rpc.CodeConflict)
	}
	b.bound, b.self = true, bd.SelfID
	return nil
}

// ApplyState makes the backend follow the snapshot. It returns only after
// sessions the snapshot no longer allows are closed: it is the revocation
// barrier.
func (b *Backend) ApplyState(ctx context.Context, s rpc.SyncState) error {
	b.mu.Lock()
	if !b.bound {
		b.mu.Unlock()
		return rpc.NewError(rpc.CodeInvalidState)
	}
	b.st = s
	b.mu.Unlock()
	if s.Runtime == "active" {
		if err := b.up(ctx); err != nil {
			return err
		}
	} else {
		b.down()
	}
	b.reconcile()
	return nil
}

// up starts the transport (first time only) and the listener.
func (b *Backend) up(ctx context.Context) error {
	b.life.Lock()
	defer b.life.Unlock()
	b.mu.Lock()
	if b.run != nil {
		b.mu.Unlock()
		return nil
	}
	if !b.trUsed {
		b.trUsed, b.trTried = true, true
		b.trDone = make(chan struct{})
		done := b.trDone
		res := make(chan error, 1)
		go func() { res <- b.cfg.Transport.Start(); close(done) }()
		b.mu.Unlock()
		select {
		case err := <-res:
			if err != nil {
				b.mu.Lock()
				b.trUsed = false // a later state tries again
				b.mu.Unlock()
				return rpc.NewError(rpc.CodeUnavailable)
			}
		case <-ctx.Done():
			return rpc.NewError(rpc.CodeCancelled)
		}
		b.mu.Lock()
	}
	b.mu.Unlock()
	rctx, cancel := context.WithCancel(context.Background())
	ln, err := b.cfg.Transport.Listen(rctx)
	if err != nil {
		cancel()
		return rpc.NewError(rpc.CodeUnavailable)
	}
	r := &runState{ctx: rctx, cancel: cancel, ln: ln, conns: map[net.Conn]struct{}{}}
	b.mu.Lock()
	b.run = r
	b.mu.Unlock()
	b.spawn(r, func() { b.acceptLoop(r) })
	return nil
}

// down closes the listener, every connection and every goroutine of the run.
// The transport stays up: the tunnel is restarted only by a new process.
func (b *Backend) down() {
	b.life.Lock()
	defer b.life.Unlock()
	b.mu.Lock()
	r := b.run
	b.run = nil
	ss := make([]*session, 0, len(b.sessions))
	for _, s := range b.sessions {
		ss = append(ss, s)
	}
	for id, d := range b.dialers {
		d.cancel()
		delete(b.dialers, id)
	}
	var conns []net.Conn
	if r != nil {
		for c := range r.conns {
			conns = append(conns, c)
		}
	}
	b.mu.Unlock()
	if r == nil {
		return
	}
	r.cancel()
	_ = r.ln.Close()
	for _, s := range ss {
		s.close()
	}
	for _, c := range conns {
		_ = c.Close()
	}
	r.wg.Wait()
}

// Close takes everything down, including the transport.
func (b *Backend) Close(context.Context) error {
	b.down()
	b.mu.Lock()
	used, done := b.trTried, b.trDone
	b.mu.Unlock()
	if !used {
		return nil
	}
	// Close first: a Start still waiting for the network is cut short by it.
	err := b.cfg.Transport.Close()
	<-done
	return err
}

// spawn runs f as a goroutine of run r; it reports false once r is over.
func (b *Backend) spawn(r *runState, f func()) bool {
	b.mu.Lock()
	if b.run != r || r.ctx.Err() != nil {
		b.mu.Unlock()
		return false
	}
	r.wg.Add(1)
	b.mu.Unlock()
	go func() { defer r.wg.Done(); f() }()
	return true
}

// track registers a connection that is still being set up so that down can
// cut it short.
func (b *Backend) track(r *runState, c net.Conn) (untrack func(), ok bool) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.run != r || r.ctx.Err() != nil {
		_ = c.Close()
		return func() {}, false
	}
	r.conns[c] = struct{}{}
	return func() { b.mu.Lock(); delete(r.conns, c); b.mu.Unlock() }, true
}

func (b *Backend) running() *runState { b.mu.Lock(); defer b.mu.Unlock(); return b.run }

// ---- pairing ----

// PairingAddress is the address a ticket carries: this device's own.
func (b *Backend) PairingAddress(context.Context, string) (string, error) {
	if b.running() == nil {
		return "", rpc.NewError(rpc.CodeUnavailable)
	}
	addr := b.cfg.Transport.Addr()
	if addr == "" {
		return "", rpc.NewError(rpc.CodeUnavailable)
	}
	return addr, nil
}

// Connect makes one connection to c.Address as the Engine's explicit attempt.
// A session is made only if the device is allowed on both sides.
func (b *Backend) Connect(ctx context.Context, c rpc.Connect) (rpc.ConnectResult, error) {
	r := b.running()
	if r == nil {
		return rpc.ConnectResult{}, rpc.NewError(rpc.CodeUnavailable)
	}
	if c.Address == b.cfg.Transport.Addr() {
		return rpc.ConnectResult{}, rpc.NewError(rpc.CodeInvalidArgument)
	}
	conn, err := b.cfg.Transport.Dial(ctx, c.Address)
	if err != nil {
		if errors.Is(err, ErrBadAddress) {
			return rpc.ConnectResult{}, rpc.NewError(rpc.CodeInvalidArgument)
		}
		return rpc.ConnectResult{}, rpc.NewError(rpc.CodeUnavailable)
	}
	untrack, ok := b.track(r, conn)
	if !ok {
		return rpc.ConnectResult{}, rpc.NewError(rpc.CodeUnavailable)
	}
	res, err := b.handshake(ctx, r, conn, true, c.Address, c.ExpectedPeerID, &c.AttemptID)
	untrack()
	if err != nil {
		_ = conn.Close()
		return rpc.ConnectResult{}, asRPC(err)
	}
	if res.sess != nil {
		if !b.establish(r, res.sess) {
			res.remote = true // a better session to this peer exists already
		}
	} else {
		_ = conn.Close()
	}
	return rpc.ConnectResult{PeerID: res.peerID, AwaitingRemoteApproval: !res.remote}, nil
}

// asRPC maps a handshake failure to an RPC error without forwarding text.
func asRPC(err error) error {
	var re *rpc.Error
	switch {
	case errors.As(err, &re):
		return re
	case errors.Is(err, ErrProtocol):
		return rpc.NewError("protocol_mismatch")
	}
	return rpc.NewError(rpc.CodeUnavailable)
}

// ---- handshake ----

type hsResult struct {
	peerID string
	remote bool     // the device allows us
	sess   *session // set when both allow
}

// handshake exchanges hellos, then identity proofs, then verdicts, with the
// Engine's authority doing every check. expect is the DeviceID the peer must
// prove ("" accepts any); attempt is the Engine's explicit attempt, if any.
func (b *Backend) handshake(parent context.Context, r *runState, conn net.Conn, outgoing bool, dialed, expect string, attempt *string) (*hsResult, error) {
	auth := b.authority()
	if auth == nil {
		return nil, rpc.NewError(rpc.CodeUnavailable)
	}
	_ = conn.SetDeadline(time.Now().Add(handshakeTimeout))
	defer func() { _ = conn.SetDeadline(time.Time{}) }()
	ctx, cancel := context.WithTimeout(r.ctx, handshakeTimeout)
	defer cancel()
	stop := context.AfterFunc(parent, cancel)
	defer stop()

	own := b.cfg.Transport.Addr()
	bindAddr, dir := own, "incoming"
	if outgoing {
		bindAddr, dir = dialed, "outgoing"
	}
	binding, err := b.cfg.Transport.Binding(bindAddr)
	if err != nil {
		return nil, fmt.Errorf("handshake: %w", err)
	}
	connID := randHex(16)
	hr, err := auth.IdentityHello(ctx, rpc.IdentityHello{ConnectionID: connID, Direction: dir, AttemptID: attempt, ContextBinding: b64.EncodeToString(binding)})
	if err != nil {
		return nil, err
	}
	mine, err := b64.DecodeString(hr.Hello)
	if err != nil {
		return nil, rpc.NewError(rpc.CodeInternal)
	}
	w := wire.NewConn(conn)
	werr := make(chan error, 1)
	go func() {
		werr <- w.WriteJSON(wire.Hello{T: "hello", Version: wire.Version, H: mine, Name: b.selfName(), Addr: own})
	}()
	var h wire.Hello
	if err := readTyped(w, "hello", &h); err != nil {
		return nil, err
	}
	if err := <-werr; err != nil {
		return nil, fmt.Errorf("handshake: %w", err)
	}
	if h.Version != wire.Version {
		return nil, ErrProtocol
	}
	pr, err := auth.IdentityProve(ctx, rpc.IdentityProve{ConnectionID: connID, PeerHello: b64.EncodeToString(h.H)})
	if err != nil {
		return nil, err
	}
	sig, err := b64.DecodeString(pr.Proof)
	if err != nil {
		return nil, rpc.NewError(rpc.CodeInternal)
	}
	proof := wire.Proof{T: "proof", Sig: sig}
	if pr.PairProof != nil {
		if proof.Pair, err = b64.DecodeString(*pr.PairProof); err != nil {
			return nil, rpc.NewError(rpc.CodeInternal)
		}
	}
	go func() { werr <- w.WriteJSON(proof) }()
	var p wire.Proof
	if err := readTyped(w, "proof", &p); err != nil {
		return nil, err
	}
	if err := <-werr; err != nil {
		return nil, fmt.Errorf("handshake: %w", err)
	}
	peerAddr := h.Addr
	if outgoing {
		peerAddr = dialed // what worked is more reliable than what the peer reports
	}
	if len(peerAddr) > rpc.MaxPeerAddress {
		peerAddr = ""
	}
	cp := rpc.CheckPeer{ConnectionID: connID, Direction: dir, AttemptID: attempt,
		Hello:     rpc.PeerWire{WireVersion: rpc.WireVersion, Name: h.Name, Address: peerAddr},
		PeerProof: b64.EncodeToString(p.Sig)}
	if len(p.Pair) > 0 {
		pp := b64.EncodeToString(p.Pair)
		cp.PeerPairProof = &pp
	}
	dec, err := auth.CheckPeer(ctx, cp)
	if err != nil {
		return nil, err
	}
	local := dec.Decision == "allow"
	res := &hsResult{peerID: dec.PeerID}
	go func() { werr <- w.WriteJSON(wire.Verdict{T: "verdict", OK: local}) }()
	var vd wire.Verdict
	rerr := readTyped(w, "verdict", &vd)
	if err := <-werr; err != nil && rerr == nil {
		rerr = fmt.Errorf("handshake: %w", err)
	}
	var s *session
	if local && dec.SessionToken != nil {
		s = &session{b: b, r: r, conn: conn, w: w, connID: connID, token: *dec.SessionToken, peerID: dec.PeerID, outgoing: outgoing,
			trustRev: uint64(dec.TrustRevision), done: make(chan struct{}), exited: make(chan struct{})}
	}
	if rerr != nil {
		s.abort()
		return nil, rerr
	}
	res.remote = vd.OK
	if expect != "" && dec.PeerID != expect {
		s.abort()
		return nil, rpc.NewError("identity_mismatch")
	}
	if s != nil && !vd.OK {
		s.abort()
		s = nil
	}
	res.sess = s
	return res, nil
}

func (b *Backend) selfName() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.st.DeviceName
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

func randHex(n int) string {
	buf := make([]byte, n)
	_, _ = rand.Read(buf)
	return hex.EncodeToString(buf)
}

func (b *Backend) acceptLoop(r *runState) {
	for {
		conn, err := r.ln.Accept()
		if err != nil {
			if r.ctx.Err() == nil {
				b.logf("accept: %v", err)
			}
			return
		}
		untrack, ok := b.track(r, conn)
		if !ok {
			return
		}
		if !b.spawn(r, func() {
			res, err := b.handshake(r.ctx, r, conn, false, "", "", nil)
			untrack()
			if err != nil || res.sess == nil {
				_ = conn.Close() // failed, pending, rejected or not accepted by the peer
				return
			}
			b.establish(r, res.sess)
		}) {
			untrack()
			_ = conn.Close()
			return
		}
	}
}

// ---- sessions ----

type session struct {
	b        *Backend
	r        *runState
	conn     net.Conn
	w        *wire.Conn
	connID   string
	token    string
	peerID   string
	outgoing bool
	trustRev uint64

	wmu    sync.Mutex // one writer
	done   chan struct{}
	exited chan struct{} // closed when serving ended and the Engine was told
	once   sync.Once
	seq    uint64 // last event sequence sent
}

func (s *session) isDone() bool {
	select {
	case <-s.done:
		return true
	default:
		return false
	}
}

func (s *session) close() {
	s.once.Do(func() {
		close(s.done)
		_ = s.conn.Close()
	})
}

// event reports a connection state to the Engine.
func (s *session) event(state string) error {
	ctx, cancel := context.WithTimeout(context.Background(), eventTimeout)
	defer cancel()
	s.seq++
	return s.b.authority().PeerEvent(ctx, rpc.PeerEvent{ConnectionID: s.connID, SessionToken: s.token, Sequence: rpc.U64(s.seq), State: state, RemoteAccepted: state == "connected"})
}

// abort ends a session that never connected, so the Engine drops its token.
func (s *session) abort() {
	if s == nil {
		return
	}
	s.close()
	_ = s.event("disconnected")
	close(s.exited)
}

// dialerOf is the device that opened the connection; of two simultaneous
// connections to one peer the one dialed by the smaller device ID survives,
// the rule the Engine applies too (both ends agree without talking).
func (b *Backend) dialerOf(s *session) string {
	if s.outgoing {
		return b.self
	}
	return s.peerID
}

// establish makes s the session of its peer, unless a better one exists, and
// reports the connection to the Engine. It reports false when s was dropped.
func (b *Backend) establish(r *runState, s *session) bool {
	b.mu.Lock()
	old := b.sessions[s.peerID]
	if old != nil && !old.isDone() && b.dialerOf(old) <= b.dialerOf(s) {
		b.mu.Unlock()
		s.abort()
		return false
	}
	b.mu.Unlock()
	if err := s.event("connected"); err != nil {
		s.close()
		close(s.exited)
		return false
	}
	b.mu.Lock()
	if b.run != r || r.ctx.Err() != nil {
		b.mu.Unlock()
		s.close()
		_ = s.event("disconnected")
		close(s.exited)
		return false
	}
	replaced := b.sessions[s.peerID]
	b.sessions[s.peerID] = s
	r.wg.Add(1)
	b.mu.Unlock()
	if replaced != nil {
		replaced.close() // its serve goroutine reports the end
	}
	go func() { defer r.wg.Done(); b.serve(s) }()
	return true
}

// serve runs a registered session until it ends.
func (b *Backend) serve(s *session) {
	err := b.recvLoop(s)
	s.close()
	b.mu.Lock()
	if b.sessions[s.peerID] == s {
		delete(b.sessions, s.peerID)
	}
	b.mu.Unlock()
	_ = s.event("disconnected")
	close(s.exited)
	if err != nil && !errors.Is(err, io.EOF) && !errors.Is(err, net.ErrClosed) {
		b.logf("session ended: %v", err)
	}
	b.kick(s.peerID)
}

// ---- sending ----

// Send writes one clip to the connection s names. The Engine authorizes it
// last, immediately before the first byte, and hands out each part once.
func (b *Backend) Send(ctx context.Context, m rpc.Send, src plugin.SendSource) error {
	b.mu.Lock()
	sess := b.sessions[m.PeerID]
	b.mu.Unlock()
	if sess == nil || sess.isDone() {
		return rpc.NewError(rpc.CodeNotFound)
	}
	if sess.connID != m.ConnectionID || subtle.ConstantTimeCompare([]byte(sess.token), []byte(m.SessionToken)) != 1 {
		return rpc.NewError(rpc.CodeConflict)
	}
	g, err := src.Authorize(ctx, rpc.Authorize{DispatchID: m.DispatchID, ContentHandle: m.ContentHandle, ConnectionID: m.ConnectionID, SessionToken: m.SessionToken})
	if err != nil {
		return err
	}
	var open []io.Closer
	defer func() {
		for _, c := range open {
			_ = c.Close()
		}
	}()
	out := make([]wire.OutPart, len(m.Clip.Parts))
	var srcErr error
	for i, p := range m.Clip.Parts {
		rc, err := src.OpenPart(ctx, g.GrantID, i)
		if err != nil {
			return err
		}
		open = append(open, rc)
		op := wire.OutPart{Size: int64(p.Size), R: &srcReader{r: rc, fail: &srcErr}}
		if p.Kind == "file" {
			op.Kind, op.Name = "file", p.Name
		} else {
			op.Kind, op.Name = "fmt", p.MIME
		}
		out[i] = op
	}
	sess.wmu.Lock()
	defer sess.wmu.Unlock()
	stop := context.AfterFunc(ctx, sess.close) // a cancelled send leaves the stream out of step
	defer stop()
	if err := sess.w.SendStream(m.Clip.WireID, m.Clip.Origin, int64(m.Clip.AtMs), m.Clip.Hash, out); err != nil {
		sess.close() // the stream may be out of step: start over
		if srcErr != nil {
			return srcErr // the content failed, not the network
		}
		return rpc.NewError(rpc.CodeUnavailable)
	}
	return nil
}

// ---- receiving ----

func (b *Backend) recvLoop(s *session) error {
	for {
		typ, raw, err := s.w.ReadRaw()
		if err != nil {
			return err
		}
		if typ != "clip" {
			continue // unknown control messages are ignored for forward compatibility
		}
		var m wire.Clip
		if err := json.Unmarshal(raw, &m); err != nil {
			return err
		}
		if err := m.Validate(); err != nil {
			return err
		}
		if err := b.receive(s, &m); err != nil {
			return err
		}
	}
}

// receive hands the clip header to the Engine and, if admitted, the bodies.
// A clip nobody wants is read and dropped so the stream stays in sync.
func (b *Backend) receive(s *session, m *wire.Clip) error {
	rcv := b.receiver()
	if rcv == nil {
		return s.w.Discard(m)
	}
	o := rpc.ReceiveOpen{ConnectionID: s.connID, SessionToken: s.token,
		Clip: rpc.Clip{WireID: m.ID, Origin: m.Origin, AtMs: rpc.U64(max(m.TS, 0)), Hash: m.Hash}}
	for i, p := range m.Parts {
		cp := rpc.ClipPart{Index: i, Size: rpc.U64(p.Size)}
		if p.Kind == "file" {
			cp.Kind, cp.Name = "file", p.Name
		} else {
			cp.Kind, cp.MIME = "representation", p.Name
		}
		o.Clip.Parts = append(o.Clip.Parts, cp)
	}
	if o.Validate() != nil {
		return s.w.Discard(m) // malformed for the Engine: read and drop
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go func() {
		select {
		case <-s.done:
			cancel()
		case <-ctx.Done():
		}
	}()
	g, err := rcv.OpenReceive(ctx, o)
	if err != nil || g.Decision != rpc.DecisionAccept || g.ReceiveID == nil {
		return s.w.Discard(m)
	}
	parts := &recvParts{w: s.w, m: m}
	_, _ = rcv.Receive(ctx, *g.ReceiveID, parts) // a clip the Engine refused is dropped; the connection stays
	return parts.drain()
}

// recvParts serves the bodies of an admitted clip to the Engine in order.
type recvParts struct {
	w    *wire.Conn
	m    *wire.Clip
	next int
	cur  *recvBody
}

type recvBody struct {
	r      io.Reader
	left   int64
	closed bool
}

func (b *recvBody) Read(p []byte) (int, error) {
	if b.closed {
		return 0, io.ErrClosedPipe
	}
	n, err := b.r.Read(p)
	b.left -= int64(n)
	if err == io.EOF && b.left > 0 {
		err = io.ErrUnexpectedEOF
	}
	return n, err
}

func (b *recvBody) Close() error { b.closed = true; return nil }

func (p *recvParts) Next(ctx context.Context) (int, io.ReadCloser, error) {
	if err := p.finishCur(); err != nil {
		return 0, nil, err
	}
	if err := ctx.Err(); err != nil {
		return 0, nil, err
	}
	if p.next >= len(p.m.Parts) {
		return 0, nil, io.EOF
	}
	i := p.next
	p.next++
	size := p.m.Parts[i].Size
	p.cur = &recvBody{r: p.w.Body(size), left: size}
	return i, p.cur, nil
}

// finishCur discards what the Engine left unread of the current body.
func (p *recvParts) finishCur() error {
	if p.cur == nil {
		return nil
	}
	c := p.cur
	p.cur = nil
	if c.left > 0 {
		n, err := io.CopyN(io.Discard, c.r, c.left)
		c.left -= n
		return err
	}
	return nil
}

// drain reads and drops every part the Engine did not take.
func (p *recvParts) drain() error {
	if err := p.finishCur(); err != nil {
		return err
	}
	for ; p.next < len(p.m.Parts); p.next++ {
		n := p.m.Parts[p.next].Size
		if _, err := io.CopyN(io.Discard, p.w.Body(n), n); err != nil {
			return err
		}
	}
	return nil
}

// ---- following the state ----

// dialer keeps one outgoing connection to a peer, with backoff.
type dialer struct {
	addr   string
	cancel context.CancelFunc
	wake   chan struct{}
}

// connectedLocked: a live session with the peer exists. b.mu must be held.
func (b *Backend) connectedLocked(id string) bool {
	s := b.sessions[id]
	return s != nil && !s.isDone()
}

// kick makes the peer's dialer look again at once.
func (b *Backend) kick(id string) {
	b.mu.Lock()
	d := b.dialers[id]
	b.mu.Unlock()
	if d != nil {
		select {
		case d.wake <- struct{}{}:
		default:
		}
	}
}

// reconcile makes the connections match the latest state: devices it no longer
// lists (or lists under a changed authority) lose their session before this
// returns, and every listed device with an address is dialed.
func (b *Backend) reconcile() {
	b.mu.Lock()
	r, st := b.run, b.st
	if r == nil {
		b.mu.Unlock()
		return
	}
	want := make(map[string]rpc.StatePeer, len(st.Peers))
	for _, p := range st.Peers {
		want[p.ID] = p
	}
	genChanged := uint64(st.NetworkGeneration) != b.gen
	b.gen = uint64(st.NetworkGeneration)
	var drop []*session
	for id, s := range b.sessions {
		if p, ok := want[id]; !ok || uint64(p.TrustRevision) != s.trustRev {
			drop = append(drop, s)
		}
	}
	for id, d := range b.dialers {
		if p, ok := want[id]; !ok || p.Address != d.addr || (genChanged && !b.connectedLocked(id)) {
			d.cancel()
			delete(b.dialers, id)
		}
	}
	var start []rpc.StatePeer
	for _, p := range st.Peers {
		if p.Address != "" && b.dialers[p.ID] == nil {
			start = append(start, p)
		}
	}
	b.mu.Unlock()
	for _, s := range drop {
		s.close()
		<-s.exited // the Engine has been told; nothing of it runs on
	}
	for _, p := range start {
		b.keepDialing(r, p)
	}
}

// keepDialing maintains an outgoing connection to p, with backoff.
func (b *Backend) keepDialing(r *runState, p rpc.StatePeer) {
	ctx, cancel := context.WithCancel(r.ctx)
	d := &dialer{addr: p.Address, cancel: cancel, wake: make(chan struct{}, 1)}
	b.mu.Lock()
	if b.run != r || b.dialers[p.ID] != nil {
		b.mu.Unlock()
		cancel()
		return
	}
	b.dialers[p.ID] = d
	b.mu.Unlock()
	if !b.spawn(r, func() {
		backoff := time.Second
		for ctx.Err() == nil {
			b.mu.Lock()
			up := b.connectedLocked(p.ID)
			b.mu.Unlock()
			if up {
				if !wait(ctx, d.wake, 5*time.Second) {
					return
				}
				backoff = time.Second
				continue
			}
			if b.dialOnce(ctx, r, p) {
				backoff = time.Second
				continue
			}
			if !wait(ctx, d.wake, backoff) {
				return
			}
			if backoff < time.Minute {
				backoff *= 2
			}
		}
	}) {
		cancel()
		b.mu.Lock()
		if b.dialers[p.ID] == d {
			delete(b.dialers, p.ID)
		}
		b.mu.Unlock()
	}
}

// dialOnce makes one attempt and reports whether a session came up.
func (b *Backend) dialOnce(ctx context.Context, r *runState, p rpc.StatePeer) bool {
	dctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	conn, err := b.cfg.Transport.Dial(dctx, p.Address)
	cancel()
	if err != nil {
		return false
	}
	untrack, ok := b.track(r, conn)
	if !ok {
		return false
	}
	res, err := b.handshake(ctx, r, conn, true, p.Address, p.ID, nil)
	untrack()
	if err != nil || res.sess == nil {
		_ = conn.Close()
		return false
	}
	return b.establish(r, res.sess)
}

func wait(ctx context.Context, wake <-chan struct{}, d time.Duration) bool {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-wake:
		return true
	case <-t.C:
		return true
	}
}

// srcReader remembers the first error of a content source, so a failure of the
// content can be told from a failure of the connection.
type srcReader struct {
	r    io.Reader
	fail *error
}

func (s *srcReader) Read(p []byte) (int, error) {
	n, err := s.r.Read(p)
	if err != nil && err != io.EOF && *s.fail == nil {
		*s.fail = err
	}
	return n, err
}
