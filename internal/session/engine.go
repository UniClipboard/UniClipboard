// Package session is the peer-session layer of the built-in tailcat backend.
// It keeps a connection to every device the Engine lists in its state
// snapshot, carries the identity handshake (docs/contracts/identity-protocol.md)
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
	"slices"
	"sync"
	"sync/atomic"
	"time"

	"github.com/uniclipboard/unic/internal/syncdiag"
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
	Observer       syncdiag.Observer
	Transport      Transport
	Logf           func(format string, args ...any)
	ProbeTimeout   time.Duration // optional override for ping liveness timeout (defaults to 4s)
	AckTimeout     time.Duration // optional override for clip_ack wait timeout (defaults to 30s)
	DisableClipAck bool          // test-only: omit "clip_ack" from wire.Hello.Caps to exercise legacy peers
}

var (
	// ErrProtocol: the peer speaks another wire version.
	ErrProtocol = errors.New("peer speaks another protocol version")
	// ErrBadAddress: a peer address the transport cannot use.
	ErrBadAddress = errors.New("invalid peer address")
	b64           = base64.RawURLEncoding
)

const (
	handshakeTimeout    = 15 * time.Second
	eventTimeout        = 5 * time.Second
	defaultAckTimeout   = 30 * time.Second
	progressMinInterval = 200 * time.Millisecond
	progressMinBytes    = 256 << 10
	maxAckIDBytes       = 128
)

// Backend is the session layer. The Engine binds it, then drives it with
// state snapshots: the network is up exactly while the latest state is active.
type Backend struct {
	closing bool
	cfg     Config
	auth    plugin.PeerAuthority
	recv    plugin.PeerReceiver

	life sync.Mutex // serializes bringing the network up and down
	// A connected callback publishes the send target before it returns.
	// Keep session election/registration atomic with respect to send lookup.
	publication sync.RWMutex

	mu           sync.Mutex
	bound        bool
	self         string
	st           rpc.SyncState
	trTried      bool          // Transport.Start was ever called; Close must release it
	trUsed       bool          // Transport.Start succeeded or is currently in flight
	trReady      bool          // Transport.Start succeeded
	trDone       chan struct{} // closed when the latest Transport.Start call returned
	run          *runState     // non-nil while the network is up
	recoveries   map[string]recoveryObservation
	sessions     map[string]*session
	dialers      map[string]*dialer
	dialing      map[string]dialStatus
	lastChecked   map[string]int64
	onLinkChange  func(string)
	relayProvide      func(peerID string) (plugin.RelayProfile, bool)
	onRelayPeer       func(peerID string, p plugin.RelayProfile)
	onHandoffOffer    func(peerID string, offer plugin.RelayHandoffOffer) plugin.RelayHandoffAck
	onHandoffChunk    func(peerID, transferID string, index int, ciphertext []byte)
	onHandoffPeerUp   func(peerID string, handoffCap, clipAckCap bool)
	onHandoffPeerDown func(peerID string)
	gen               uint64
}

// runState is one up period of the network.
type runState struct {
	ctx     context.Context
	cancel  context.CancelFunc
	mu      sync.Mutex // guards ln
	ln      net.Listener
	serving bool                  // guarded by Backend.mu
	conns   map[net.Conn]struct{} // connections still handshaking
	wg      sync.WaitGroup
}

func New(cfg Config) *Backend {
	if cfg.Logf == nil {
		cfg.Logf = func(string, ...any) {} // session logs name devices; never default to the global logger
	}
	return &Backend{
		cfg:         cfg,
		recoveries:  map[string]recoveryObservation{},
		sessions:    map[string]*session{},
		dialers:     map[string]*dialer{},
		dialing:     map[string]dialStatus{},
		lastChecked: map[string]int64{},
	}
}

func (b *Backend) logf(f string, a ...any) { b.cfg.Logf(f, a...) }

func (b *Backend) SetPeerAuthority(a plugin.PeerAuthority) { b.mu.Lock(); b.auth = a; b.mu.Unlock() }
func (b *Backend) SetPeerReceiver(r plugin.PeerReceiver)   { b.mu.Lock(); b.recv = r; b.mu.Unlock() }

func (b *Backend) SetRelayCallbacks(
	provide func(peerID string) (plugin.RelayProfile, bool),
	onPeerProfile func(peerID string, p plugin.RelayProfile),
) {
	b.mu.Lock()
	b.relayProvide = provide
	b.onRelayPeer = onPeerProfile
	b.mu.Unlock()
}

func (b *Backend) PushRelayProfiles() {
	b.mu.Lock()
	var ss []*session
	for _, s := range b.sessions {
		if s.relayProfileCap && !s.isDone() {
			ss = append(ss, s)
		}
	}
	b.mu.Unlock()
	for _, s := range ss {
		s.wakeRelayProfile()
	}
}

func (b *Backend) WakePeer(peerID string) {
	if peerID != "" {
		b.kick(peerID)
	}
}

func (b *Backend) SetRelayHandoffCallbacks(
	onOffer func(peerID string, offer plugin.RelayHandoffOffer) plugin.RelayHandoffAck,
	onChunk func(peerID, transferID string, index int, ciphertext []byte),
	onPeerConnected func(peerID string, handoffCap, clipAckCap bool),
	onPeerDisconnected func(peerID string),
) {
	b.mu.Lock()
	b.onHandoffOffer = onOffer
	b.onHandoffChunk = onChunk
	b.onHandoffPeerUp = onPeerConnected
	b.onHandoffPeerDown = onPeerDisconnected
	b.mu.Unlock()
}

func (b *Backend) PeerHandoffCaps(peerID string) (connected, handoffCap, clipAckCap bool) {
	b.mu.Lock()
	s := b.sessions[peerID]
	b.mu.Unlock()
	if s == nil || s.isDone() {
		return false, false, false
	}
	return true, s.relayHandoffCap, s.clipAckCap
}

func (b *Backend) RegisterRelayClipAckWaiter(peerID, transferID string) (<-chan string, <-chan struct{}, func(), bool) {
	b.mu.Lock()
	s := b.sessions[peerID]
	b.mu.Unlock()
	if s == nil || s.isDone() || !s.clipAckCap {
		return nil, nil, func() {}, false
	}
	ch, cleanup := s.registerAckWaiter(transferID)
	return ch, s.done, cleanup, true
}

func (b *Backend) SendRelayHandoffOffer(ctx context.Context, peerID string, offer plugin.RelayHandoffOffer) (plugin.RelayHandoffAck, error) {
	b.mu.Lock()
	s := b.sessions[peerID]
	b.mu.Unlock()
	if s == nil || s.isDone() || !s.relayHandoffCap {
		return plugin.RelayHandoffAck{}, rpc.NewError(rpc.CodeUnavailable)
	}
	ch, cleanup := s.registerHandoffWaiter(offer.TransferID)
	defer cleanup()

	writeCtx, writeCancel := context.WithTimeout(ctx, eventTimeout)
	defer writeCancel()
	if !s.acquireWrite(writeCtx) {
		if ctx.Err() != nil {
			return plugin.RelayHandoffAck{}, rpc.NewError(rpc.CodeCancelled)
		}
		return plugin.RelayHandoffAck{}, rpc.NewError(rpc.CodeUnavailable)
	}
	_ = s.conn.SetWriteDeadline(time.Now().Add(eventTimeout))
	err := s.w.WriteJSON(wire.RelayHandoffOffer{
		T:                   "relay_handoff_offer",
		V:                   1,
		TransferID:          offer.TransferID,
		Envelope:            json.RawMessage(offer.EnvelopeJSON),
		RelayUploadedBitmap: offer.RelayUploadedBitmap,
	})
	_ = s.conn.SetWriteDeadline(time.Time{})
	s.releaseWrite()
	if err != nil {
		s.closeWith(reason(err))
		return plugin.RelayHandoffAck{}, rpc.NewError(rpc.CodeUnavailable)
	}
	s.markTx()

	timer := time.NewTimer(eventTimeout)
	defer timer.Stop()
	select {
	case ack := <-ch:
		return ack, nil
	case <-s.done:
		return plugin.RelayHandoffAck{}, rpc.NewError(rpc.CodeUnavailable)
	case <-ctx.Done():
		return plugin.RelayHandoffAck{}, rpc.NewError(rpc.CodeCancelled)
	case <-timer.C:
		return plugin.RelayHandoffAck{}, rpc.NewError(rpc.CodeTimeout)
	}
}

func (b *Backend) SendRelayChunk(ctx context.Context, peerID, transferID string, index int, ciphertext []byte) error {
	b.mu.Lock()
	s := b.sessions[peerID]
	b.mu.Unlock()
	if s == nil || s.isDone() || !s.relayHandoffCap {
		return rpc.NewError(rpc.CodeUnavailable)
	}
	if !s.acquireWrite(ctx) {
		if ctx.Err() != nil {
			return rpc.NewError(rpc.CodeCancelled)
		}
		return rpc.NewError(rpc.CodeUnavailable)
	}
	s.bodyActive.Store(true)
	if s.bodyConn != nil {
		s.bodyConn.writingBody = true
	}
	stop := context.AfterFunc(ctx, func() { s.closeWith("cancellation") })
	err := s.w.SendRelayChunk(transferID, index, ciphertext)
	stop()
	if s.bodyConn != nil {
		s.bodyConn.writingBody = false
		_ = s.conn.SetWriteDeadline(time.Time{})
	}
	s.bodyActive.Store(false)
	s.releaseWrite()
	if err != nil {
		if ctx.Err() != nil {
			s.closeWith("cancellation")
			return rpc.NewError(rpc.CodeCancelled)
		}
		s.closeWith(reason(err))
		return rpc.NewError(rpc.CodeUnavailable)
	}
	s.markTx()
	return nil
}

func (b *Backend) SendRelayClipAck(peerID, transferID, status string) bool {
	b.mu.Lock()
	s := b.sessions[peerID]
	b.mu.Unlock()
	if s == nil || s.isDone() || !s.clipAckCap || transferID == "" {
		return false
	}
	s.enqueueAck(wire.ClipAck{T: "clip_ack", ID: transferID, Status: status})
	return true
}

func (b *Backend) authority() plugin.PeerAuthority { b.mu.Lock(); defer b.mu.Unlock(); return b.auth }
func (b *Backend) receiver() plugin.PeerReceiver   { b.mu.Lock(); defer b.mu.Unlock(); return b.recv }

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
	b.mu.Lock()
	for _, sess := range b.sessions {
		sess.wakeIntroductions()
	}
	b.mu.Unlock()
	return nil
}

// up activates the run immediately and starts the transport (first time only)
// and listener asynchronously so local startup never blocks on network probes.
func (b *Backend) up(_ context.Context) error {
	b.life.Lock()
	defer b.life.Unlock()
	b.mu.Lock()
	r := b.run
	if r == nil {
		rctx, cancel := context.WithCancel(context.Background())
		r = &runState{ctx: rctx, cancel: cancel, conns: map[net.Conn]struct{}{}}
		b.run = r
	}
	if !b.trReady && !b.trUsed {
		b.trUsed, b.trTried = true, true
		done := make(chan struct{})
		b.trDone = done
		go b.startTransport(done)
	}
	needServe := !r.serving
	if needServe {
		r.serving = true
	}
	b.mu.Unlock()
	if needServe {
		if !b.spawn(r, func() { b.serveRun(r) }) {
			b.mu.Lock()
			if b.run == r {
				r.serving = false
			}
			b.mu.Unlock()
		}
	}
	return nil
}

func (b *Backend) startTransport(done chan struct{}) {
	epoch, start := b.epoch(), time.Now()
	err := b.cfg.Transport.Start()
	b.attempt(epoch, "network_start", "none", start, err, "")
	b.mu.Lock()
	if err == nil {
		b.trReady = true
	} else {
		b.trUsed = false // a later active state tries again
	}
	b.mu.Unlock()
	close(done)
}

func (b *Backend) waitTransport(ctx context.Context) error {
	b.mu.Lock()
	ready, used, done := b.trReady, b.trUsed, b.trDone
	b.mu.Unlock()
	if ready {
		return nil
	}
	if !used || done == nil {
		return rpc.NewError(rpc.CodeUnavailable)
	}
	select {
	case <-done:
		b.mu.Lock()
		ready = b.trReady
		b.mu.Unlock()
		if !ready {
			return rpc.NewError(rpc.CodeUnavailable)
		}
		return nil
	case <-ctx.Done():
		return rpc.NewError(rpc.CodeCancelled)
	}
}

func (b *Backend) serveRun(r *runState) {
	for {
		if err := b.waitTransport(r.ctx); err != nil {
			b.mu.Lock()
			retry := b.run == r && r.ctx.Err() == nil && (b.trReady || b.trUsed)
			if !retry && b.run == r {
				r.serving = false
			}
			b.mu.Unlock()
			if retry {
				continue
			}
			return
		}
		ln, err := b.cfg.Transport.Listen(r.ctx)
		if err != nil {
			b.mu.Lock()
			if b.run == r {
				r.serving = false
			}
			b.mu.Unlock()
			return
		}
		r.mu.Lock()
		if r.ctx.Err() != nil {
			r.mu.Unlock()
			_ = ln.Close()
			b.mu.Lock()
			if b.run == r {
				r.serving = false
			}
			b.mu.Unlock()
			return
		}
		r.ln = ln
		r.mu.Unlock()
		b.acceptLoop(r, ln)
		b.mu.Lock()
		if b.run == r {
			r.serving = false
		}
		b.mu.Unlock()
		return
	}
}

// down closes the listener, every connection and every goroutine of the run.
// The transport stays up: the tunnel is restarted only by a new process.
func (b *Backend) down() {
	epoch := b.epoch()
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
	clear(b.dialing)
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
	for _, s := range ss {
		why := "suspension"
		b.mu.Lock()
		if b.closing {
			why = "local_stop"
		}
		b.mu.Unlock()
		s.closeWith(why)
	}
	r.cancel()
	r.mu.Lock()
	ln := r.ln
	r.ln = nil
	r.mu.Unlock()
	if ln != nil {
		_ = ln.Close()
	}
	for _, c := range conns {
		_ = c.Close()
	}
	r.wg.Wait()
	b.mu.Lock()
	recoveries := b.recoveries
	b.recoveries = map[string]recoveryObservation{}
	b.mu.Unlock()
	for _, rec := range recoveries {
		if epoch != 0 && rec.epoch == epoch {
			b.emit(syncdiag.Record{Epoch: epoch, Kind: "recovery", Result: "interrupted"})
		}
	}
}

// ExpectStop marks the host's authoritative local shutdown before Engine
// withdraws active state. It changes only diagnostic classification.
func (b *Backend) ExpectStop() { b.mu.Lock(); b.closing = true; b.mu.Unlock() }

// Close takes everything down, including the transport.
func (b *Backend) Close(context.Context) error {
	b.ExpectStop()
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
func (b *Backend) PairingAddress(ctx context.Context, _ string) (string, error) {
	if b.running() == nil {
		return "", rpc.NewError(rpc.CodeUnavailable)
	}
	if err := b.waitTransport(ctx); err != nil {
		return "", err
	}
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
	if err := b.waitTransport(ctx); err != nil {
		return rpc.ConnectResult{}, err
	}
	if b.running() != r {
		return rpc.ConnectResult{}, rpc.NewError(rpc.CodeUnavailable)
	}
	if c.Address == b.cfg.Transport.Addr() {
		return rpc.ConnectResult{}, rpc.NewError(rpc.CodeInvalidArgument)
	}
	conn, err := b.dial(ctx, c.Address)
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
	peerID   string
	decision string
	remote   bool     // the device allows us
	sess     *session // set when both allow
}

// handshake exchanges hellos, then identity proofs, then verdicts, with the
// Engine's authority doing every check. expect is the DeviceID the peer must
// prove ("" accepts any); attempt is the Engine's explicit attempt, if any.
func (b *Backend) handshake(parent context.Context, r *runState, conn net.Conn, outgoing bool, dialed, expect string, attempt *string) (result *hsResult, failure error) {
	epoch, start := b.epoch(), time.Now()
	defer func() {
		dir := "incoming"
		if outgoing {
			dir = "outgoing"
		}
		status := ""
		if failure == nil && result != nil && result.sess == nil {
			status = "pending"
			if result.decision == "reject" {
				status = "rejected"
			}
		}
		b.attempt(epoch, "identity_handshake", dir, start, failure, status)
	}()

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
	bodyConn := &bodyDeadlineConn{Conn: conn}
	w := wire.NewConn(bodyConn)
	var localCaps []string
	if !b.cfg.DisableClipAck {
		localCaps = append(localCaps, wire.CapClipAck)
	}
	b.mu.Lock()
	hasRelay := b.relayProvide != nil || b.onRelayPeer != nil
	hasHandoff := b.onHandoffOffer != nil || b.onHandoffChunk != nil
	b.mu.Unlock()
	if hasRelay {
		localCaps = append(localCaps, wire.CapRelayProfile)
	}
	if hasHandoff {
		localCaps = append(localCaps, wire.CapRelayHandoff)
	}
	werr := make(chan error, 1)
	go func() {
		werr <- w.WriteJSON(wire.Hello{T: "hello", Version: wire.Version, H: mine, Name: b.selfName(), Addr: own, Caps: localCaps})
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
	res := &hsResult{peerID: dec.PeerID, decision: dec.Decision}
	go func() { werr <- w.WriteJSON(wire.Verdict{T: "verdict", OK: local}) }()
	var vd wire.Verdict
	rerr := readTyped(w, "verdict", &vd)
	if err := <-werr; err != nil && rerr == nil {
		rerr = fmt.Errorf("handshake: %w", err)
	}
	var s *session
	if local && dec.SessionToken != nil {
		clipAckCap := !b.cfg.DisableClipAck && slices.Contains(h.Caps, wire.CapClipAck)
		relayProfileCap := hasRelay && slices.Contains(h.Caps, wire.CapRelayProfile)
		relayHandoffCap := hasHandoff && slices.Contains(h.Caps, wire.CapRelayHandoff)
		s = &session{
			b: b, r: r, conn: conn, w: w, bodyConn: bodyConn, connID: connID, token: *dec.SessionToken, peerID: dec.PeerID, outgoing: outgoing,
			trustRev: uint64(dec.TrustRevision), wlock: make(chan struct{}, 1), pongSem: make(chan struct{}, 1),
			clipAckCap: clipAckCap, ackQueue: make(chan wire.ClipAck, 32),
			relayProfileCap: relayProfileCap, relayWake: make(chan struct{}, 1),
			relayHandoffCap: relayHandoffCap, handoffAckQueue: make(chan wire.RelayHandoffAck, 32),
			done: make(chan struct{}), exited: make(chan struct{}),
		}
		bodyConn.lastRxNs = &s.lastRxUnixNs
		bodyConn.lastTxNs = &s.lastTxUnixNs
		s.markRx()
		s.markTx()
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

func (b *Backend) acceptLoop(r *runState, ln net.Listener) {
	for {
		conn, err := ln.Accept()
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
	observation sessionObservation // protected by b.mu
	endMu       sync.Mutex
	endReason   string
	endEpoch    uint64
	ended       time.Time
	b           *Backend
	r           *runState
	conn        net.Conn
	w           *wire.Conn
	bodyConn    *bodyDeadlineConn
	connID      string
	token       string
	peerID      string
	outgoing    bool
	trustRev    uint64
	introWake   chan struct{}

	wlock        chan struct{} // buffered(1), serializes wire writes with cancellation support
	bodyActive   atomic.Bool
	lastRxUnixNs atomic.Int64
	lastTxUnixNs atomic.Int64
	probeMu      sync.Mutex
	probes       map[string]chan struct{}
	pongSem      chan struct{}
	pongWG       sync.WaitGroup
	clipAckCap      bool
	ackMu           sync.Mutex
	ackWaiters      map[string]chan string
	ackQueue        chan wire.ClipAck
	relayProfileCap bool
	relayWake       chan struct{}
	relayHandoffCap bool
	handoffMu       sync.Mutex
	handoffWaiters  map[string]chan plugin.RelayHandoffAck
	handoffAckQueue chan wire.RelayHandoffAck
	handoffWG       sync.WaitGroup
	link            peerLinkCache // guarded by b.mu
	done         chan struct{}
	exited       chan struct{} // closed when serving ended and the Engine was told
	once         sync.Once
	seq          uint64 // last event sequence sent
}

func (s *session) isDone() bool {
	select {
	case <-s.done:
		return true
	default:
		return false
	}
}

func (s *session) close() { s.closeWith("io_failure") }

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
	b.publication.Lock()
	defer b.publication.Unlock()
	epoch := b.epoch()
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
	now := time.Now()
	replaced := b.sessions[s.peerID]
	s.introWake = make(chan struct{}, 1)
	b.sessions[s.peerID] = s
	delete(b.dialing, s.peerID)
	s.observation = sessionObservation{epoch: epoch, started: now, observed: now}
	recovery, recovered := b.recoveries[s.peerID]
	delete(b.recoveries, s.peerID)
	r.wg.Add(1)
	b.mu.Unlock()
	if recovered && epoch != 0 {
		result := "recovered"
		duration := now.Sub(recovery.started)
		if recovery.epoch != epoch {
			result = "interrupted"
			duration = 0
		}
		b.emit(syncdiag.Record{Epoch: epoch, Kind: "recovery", Result: result, Duration: duration})
	}
	b.wakeObservation()
	if replaced != nil {
		replaced.closeWith("state_replacement") // its serve goroutine reports the end
	}
	if s.relayProfileCap {
		s.wakeRelayProfile()
	}
	b.mu.Lock()
	peerUp := b.onHandoffPeerUp
	b.mu.Unlock()
	if peerUp != nil {
		go peerUp(s.peerID, s.relayHandoffCap, s.clipAckCap)
	}
	go func() { defer r.wg.Done(); b.serve(s) }()
	return true
}

// serve runs a registered session until it ends.
func (b *Backend) serve(s *session) {
	introDone := make(chan struct{})
	go func() { defer close(introDone); s.introduceLoop() }()
	ackDone := make(chan struct{})
	go func() { defer close(ackDone); s.ackLoop() }()
	err := b.recvLoop(s)
	s.closeWith(reason(err))
	<-introDone
	<-ackDone
	s.pongWG.Wait()
	s.handoffWG.Wait()
	b.endObservation(s)
	b.mu.Lock()
	noSession := b.sessions[s.peerID] == nil
	peerDown := b.onHandoffPeerDown
	if b.run == s.r && noSession {
		ds := b.dialing[s.peerID]
		ds.attempts = 0
		ds.activeUntil = time.Now().Add(connectingGraceWindow)
		ds.visibleConnecting = true
		b.dialing[s.peerID] = ds
	}
	b.mu.Unlock()
	if noSession && peerDown != nil {
		go peerDown(s.peerID)
	}
	_ = s.event("disconnected")
	close(s.exited)
	if !errors.Is(err, io.EOF) && !errors.Is(err, net.ErrClosed) { // recvLoop always returns an error
		b.logf("session ended: %v", err)
	}
	b.kick(s.peerID)
}

// introduction is a bounded control message on an authenticated session.
// The receiver supplies its own connection ID and token, never the sender's.
type introduction struct {
	T          string          `json:"t"`
	Candidates []rpc.Candidate `json:"candidates"`
}

func (s *session) wakeIntroductions() {
	select {
	case s.introWake <- struct{}{}:
	default:
	}
}

func (s *session) introduceLoop() {
	// Renew long-lived sessions well before the seven-day candidate expiry.
	tick := time.NewTicker(24 * time.Hour)
	defer tick.Stop()
	s.wakeIntroductions()
	for {
		select {
		case <-s.done:
			return
		case <-s.r.ctx.Done():
			return
		case <-s.introWake:
		case <-tick.C:
		}
		if err := s.sendIntroductions(); err != nil {
			s.close()
			return
		}
	}
}

func (s *session) sendIntroductions() error {
	ctx := context.Background()
	if s.r != nil {
		ctx = s.r.ctx
	}
	if !s.acquireWrite(ctx) {
		return nil
	}
	defer s.releaseWrite()
	s.b.mu.Lock()
	enabled := s.b.st.Introduce && s.b.sessions[s.peerID] == s && !s.isDone()
	peers := append([]rpc.StatePeer(nil), s.b.st.Peers...)
	current := false
	for _, p := range peers {
		if p.ID == s.peerID && uint64(p.TrustRevision) == s.trustRev {
			current = true
			break
		}
	}
	enabled = enabled && current
	s.b.mu.Unlock()
	if !enabled {
		return nil
	}
	candidates := make([]rpc.Candidate, 0, len(peers))
	for _, p := range peers {
		if p.ID != s.peerID && p.ID != s.b.self && p.Address != "" {
			candidates = append(candidates, rpc.Candidate{ID: p.ID, Name: p.Name, Address: p.Address})
		}
	}
	// Keep the same writer lock across batches so clipboard bodies cannot interleave.
	for len(candidates) > 0 {
		n := min(len(candidates), rpc.MaxCandidates)
		_ = s.conn.SetWriteDeadline(time.Now().Add(eventTimeout))
		err := s.w.WriteJSON(introduction{T: "introduce", Candidates: candidates[:n]})
		_ = s.conn.SetWriteDeadline(time.Time{})
		if err != nil {
			return rpc.NewError(rpc.CodeUnavailable)
		}
		candidates = candidates[n:]
	}
	return nil
}

// ---- sending ----

func (b *Backend) ackBudget() time.Duration {
	if b.cfg.AckTimeout > 0 {
		return b.cfg.AckTimeout
	}
	return defaultAckTimeout
}

func (s *session) registerAckWaiter(id string) (chan string, func()) {
	ch := make(chan string, 1)
	s.ackMu.Lock()
	if s.ackWaiters == nil {
		s.ackWaiters = map[string]chan string{}
	}
	s.ackWaiters[id] = ch
	s.ackMu.Unlock()
	return ch, func() {
		s.ackMu.Lock()
		if s.ackWaiters[id] == ch {
			delete(s.ackWaiters, id)
		}
		s.ackMu.Unlock()
	}
}

func (s *session) handleClipAck(raw []byte) {
	var msg wire.ClipAck
	if err := json.Unmarshal(raw, &msg); err != nil || msg.ID == "" || len(msg.ID) > maxAckIDBytes {
		return
	}
	switch msg.Status {
	case wire.ClipAckReceived, wire.ClipAckDiscarded, wire.ClipAckFailed:
	default:
		return
	}
	s.ackMu.Lock()
	ch := s.ackWaiters[msg.ID]
	if ch != nil {
		delete(s.ackWaiters, msg.ID)
	}
	s.ackMu.Unlock()
	if ch != nil {
		select {
		case ch <- msg.Status:
		default:
		}
	}
}

func (s *session) enqueueAck(ack wire.ClipAck) {
	if !s.clipAckCap || s.ackQueue == nil || ack.ID == "" {
		return
	}
	select {
	case s.ackQueue <- ack:
	case <-s.done:
	default:
	}
}

func (s *session) wakeRelayProfile() {
	if !s.relayProfileCap || s.relayWake == nil {
		return
	}
	select {
	case s.relayWake <- struct{}{}:
	default:
	}
}

func (s *session) sendRelayProfile() error {
	s.b.mu.Lock()
	provide := s.b.relayProvide
	s.b.mu.Unlock()
	if provide == nil || !s.relayProfileCap {
		return nil
	}
	prof, ok := provide(s.peerID)
	if !ok || prof.RelayOrigin == "" || prof.MailboxID == "" || prof.DeliveryToken == "" {
		return nil
	}
	ctx, cancel := context.WithTimeout(s.r.ctx, eventTimeout)
	defer cancel()
	if !s.acquireWrite(ctx) {
		return nil
	}
	_ = s.conn.SetWriteDeadline(time.Now().Add(eventTimeout))
	err := s.w.WriteJSON(wire.RelayProfile{
		T:             "relay_profile",
		V:             1,
		RelayOrigin:   prof.RelayOrigin,
		MailboxID:     prof.MailboxID,
		DeliveryToken: prof.DeliveryToken,
		MaxBytes:      prof.MaxBytes,
		Epoch:         prof.Epoch,
	})
	_ = s.conn.SetWriteDeadline(time.Time{})
	s.releaseWrite()
	if err != nil {
		return err
	}
	s.markTx()
	return nil
}

func (s *session) handleRelayProfile(raw []byte) {
	if !s.relayProfileCap {
		return
	}
	var msg wire.RelayProfile
	if err := json.Unmarshal(raw, &msg); err != nil || msg.V != 1 {
		return
	}
	if len(msg.RelayOrigin) == 0 || len(msg.RelayOrigin) > 512 ||
		len(msg.MailboxID) != 32 ||
		len(msg.DeliveryToken) < 16 || len(msg.DeliveryToken) > 256 ||
		msg.MaxBytes < 0 {
		return
	}
	s.b.mu.Lock()
	cb := s.b.onRelayPeer
	s.b.mu.Unlock()
	if cb != nil {
		cb(s.peerID, plugin.RelayProfile{
			RelayOrigin:   msg.RelayOrigin,
			MailboxID:     msg.MailboxID,
			DeliveryToken: msg.DeliveryToken,
			MaxBytes:      msg.MaxBytes,
			Epoch:         msg.Epoch,
		})
	}
}

func (s *session) registerHandoffWaiter(id string) (chan plugin.RelayHandoffAck, func()) {
	ch := make(chan plugin.RelayHandoffAck, 1)
	s.handoffMu.Lock()
	if s.handoffWaiters == nil {
		s.handoffWaiters = map[string]chan plugin.RelayHandoffAck{}
	}
	s.handoffWaiters[id] = ch
	s.handoffMu.Unlock()
	return ch, func() {
		s.handoffMu.Lock()
		if s.handoffWaiters[id] == ch {
			delete(s.handoffWaiters, id)
		}
		s.handoffMu.Unlock()
	}
}

func (s *session) enqueueHandoffAck(ack wire.RelayHandoffAck) {
	if !s.relayHandoffCap || s.handoffAckQueue == nil || ack.TransferID == "" {
		return
	}
	select {
	case s.handoffAckQueue <- ack:
	case <-s.done:
	default:
	}
}

func (s *session) handleRelayHandoffOffer(raw []byte) {
	if !s.relayHandoffCap {
		return
	}
	var msg wire.RelayHandoffOffer
	if err := json.Unmarshal(raw, &msg); err != nil || msg.V != 1 || len(msg.TransferID) != 32 || len(msg.Envelope) == 0 {
		return
	}
	s.b.mu.Lock()
	cb := s.b.onHandoffOffer
	s.b.mu.Unlock()
	if cb == nil {
		s.enqueueHandoffAck(wire.RelayHandoffAck{T: "relay_handoff_ack", V: 1, TransferID: msg.TransferID, Accept: false, Reason: "unsupported"})
		return
	}
	s.handoffWG.Add(1)
	go func() {
		defer s.handoffWG.Done()
		ack := cb(s.peerID, plugin.RelayHandoffOffer{
			TransferID:          msg.TransferID,
			EnvelopeJSON:        append([]byte(nil), msg.Envelope...),
			RelayUploadedBitmap: msg.RelayUploadedBitmap,
		})
		s.enqueueHandoffAck(wire.RelayHandoffAck{
			T:             "relay_handoff_ack",
			V:             1,
			TransferID:    msg.TransferID,
			Accept:        ack.Accept,
			Reason:        ack.Reason,
			HaveBitmap:    ack.HaveBitmap,
			WantP2PBitmap: ack.WantP2PBitmap,
		})
	}()
}

func (s *session) handleRelayHandoffAck(raw []byte) {
	if !s.relayHandoffCap {
		return
	}
	var msg wire.RelayHandoffAck
	if err := json.Unmarshal(raw, &msg); err != nil || msg.V != 1 || len(msg.TransferID) != 32 {
		return
	}
	s.handoffMu.Lock()
	ch := s.handoffWaiters[msg.TransferID]
	if ch != nil {
		delete(s.handoffWaiters, msg.TransferID)
	}
	s.handoffMu.Unlock()
	if ch != nil {
		select {
		case ch <- plugin.RelayHandoffAck{
			TransferID:    msg.TransferID,
			Accept:        msg.Accept,
			Reason:        msg.Reason,
			HaveBitmap:    msg.HaveBitmap,
			WantP2PBitmap: msg.WantP2PBitmap,
		}:
		default:
		}
	}
}

func (s *session) handleRelayChunkData(raw []byte) error {
	var msg wire.RelayChunkData
	if err := json.Unmarshal(raw, &msg); err != nil {
		return err
	}
	if msg.V != 1 || len(msg.TransferID) != 32 || msg.Index < 0 || msg.Index >= wire.MaxParts || msg.Size <= 0 || msg.Size > wire.MaxRelayChunkBytes {
		return rpc.NewError(rpc.CodeInvalidArgument)
	}
	s.bodyActive.Store(true)
	defer s.bodyActive.Store(false)
	if s.bodyConn != nil {
		s.bodyConn.body = true
		defer func() { s.bodyConn.body = false; _ = s.conn.SetReadDeadline(time.Time{}) }()
	}
	buf := make([]byte, msg.Size)
	if _, err := io.ReadFull(s.w.Body(msg.Size), buf); err != nil {
		return err
	}
	s.markRx()
	if !s.relayHandoffCap {
		return nil
	}
	s.b.mu.Lock()
	cb := s.b.onHandoffChunk
	s.b.mu.Unlock()
	if cb != nil {
		cb(s.peerID, msg.TransferID, msg.Index, buf)
	}
	return nil
}

func (s *session) ackLoop() {
	if s.ackQueue == nil && s.relayWake == nil && s.handoffAckQueue == nil {
		return
	}
	for {
		select {
		case <-s.done:
			return
		case ack := <-s.ackQueue:
			ctx, cancel := context.WithTimeout(s.r.ctx, eventTimeout)
			if !s.acquireWrite(ctx) {
				cancel()
				if s.isDone() {
					return
				}
				continue
			}
			_ = s.conn.SetWriteDeadline(time.Now().Add(eventTimeout))
			err := s.w.WriteJSON(ack)
			_ = s.conn.SetWriteDeadline(time.Time{})
			s.releaseWrite()
			cancel()
			if err != nil {
				s.closeWith(reason(err))
				return
			}
			s.markTx()
		case hack := <-s.handoffAckQueue:
			ctx, cancel := context.WithTimeout(s.r.ctx, eventTimeout)
			if !s.acquireWrite(ctx) {
				cancel()
				if s.isDone() {
					return
				}
				continue
			}
			_ = s.conn.SetWriteDeadline(time.Now().Add(eventTimeout))
			err := s.w.WriteJSON(hack)
			_ = s.conn.SetWriteDeadline(time.Time{})
			s.releaseWrite()
			cancel()
			if err != nil {
				s.closeWith(reason(err))
				return
			}
			s.markTx()
		case <-s.relayWake:
			if err := s.sendRelayProfile(); err != nil {
				s.closeWith(reason(err))
				return
			}
		}
	}
}

// Send writes one clip to the connection s names. The Engine authorizes it
// last, immediately before the first byte, and hands out each part once.
func (b *Backend) Send(ctx context.Context, m rpc.Send, src plugin.SendSource) (result rpc.SendResult, failure error) {
	b.publication.RLock()
	b.mu.Lock()
	sess := b.sessions[m.PeerID]
	b.mu.Unlock()
	b.publication.RUnlock()
	if sess == nil || sess.isDone() {
		return rpc.SendResult{}, rpc.NewError(rpc.CodeNotFound)
	}
	if sess.connID != m.ConnectionID || subtle.ConstantTimeCompare([]byte(sess.token), []byte(m.SessionToken)) != 1 {
		return rpc.SendResult{}, rpc.NewError(rpc.CodeConflict)
	}
	g, err := src.Authorize(ctx, rpc.Authorize{DispatchID: m.DispatchID, ContentHandle: m.ContentHandle, ConnectionID: m.ConnectionID, SessionToken: m.SessionToken})
	if err != nil {
		return rpc.SendResult{}, err
	}
	epoch := b.epoch()
	size, kind := clipMeasurement(m.Clip.Parts)
	b.emit(syncdiag.Record{Epoch: epoch, Kind: "transfer_size", Direction: "outgoing", Declared: size, ContentKind: kind})
	startPath := sess.startPath(epoch)
	var body wire.BodyObservation
	var lastReportedBytes int64 = -1
	var lastReportedAt time.Time
	body.OnProgress = func(bytesSent int64, partIndex int) {
		if bytesSent <= lastReportedBytes || bytesSent > size {
			return
		}
		now := time.Now()
		if lastReportedBytes >= 0 && bytesSent < size && bytesSent-lastReportedBytes < progressMinBytes && now.Sub(lastReportedAt) < progressMinInterval {
			return
		}
		lastReportedBytes = bytesSent
		lastReportedAt = now
		pidx := partIndex
		_ = src.ReportProgress(ctx, rpc.SendProgress{
			GrantID:    g.GrantID,
			Stage:      rpc.SendStageSending,
			BytesSent:  rpc.U64(bytesSent),
			TotalBytes: rpc.U64(size),
			PartIndex:  &pidx,
		})
	}
	defer func() {
		res := "succeeded"
		if failure != nil {
			res = "failed"
			if ctx.Err() != nil {
				res = "cancelled"
			}
		}
		b.emit(syncdiag.Record{Epoch: epoch, Kind: "transfer", Direction: "outgoing", Result: res, Declared: size, Bytes: body.Bytes, Duration: body.Duration(), Path: startPath})
	}()
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
			return rpc.SendResult{}, err
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
	var ackCh chan string
	if sess.clipAckCap {
		var cleanupAck func()
		ackCh, cleanupAck = sess.registerAckWaiter(m.Clip.WireID)
		defer cleanupAck()
	}
	if !sess.acquireWrite(ctx) {
		if ctx.Err() != nil {
			return rpc.SendResult{}, rpc.NewError(rpc.CodeCancelled)
		}
		return rpc.SendResult{}, rpc.NewError(rpc.CodeUnavailable)
	}
	sess.bodyActive.Store(true)
	if sess.bodyConn != nil {
		sess.bodyConn.writingBody = true
	}
	stop := context.AfterFunc(ctx, func() { sess.closeWith("cancellation") }) // a cancelled write leaves the stream out of step
	writeErr := sess.w.SendApplicationStream(m.Clip.WireID, m.Clip.Origin, int64(m.Clip.AtMs), m.Clip.Hash, out, m.Clip.SourceApplication, &body)
	stop()
	if sess.bodyConn != nil {
		sess.bodyConn.writingBody = false
		_ = sess.conn.SetWriteDeadline(time.Time{})
	}
	sess.bodyActive.Store(false)
	// Release the write lock immediately once the body is written so concurrent
	// bidirectional sends and inbound clip_ack writes can never deadlock.
	sess.releaseWrite()
	if writeErr != nil {
		if srcErr != nil {
			sess.closeWith("io_failure") // the stream may be out of step: start over
			return rpc.SendResult{}, srcErr
		}
		var ne net.Error
		switch {
		case ctx.Err() != nil:
			sess.closeWith("cancellation")
			return rpc.SendResult{}, rpc.NewError(rpc.CodeCancelled)
		case errors.Is(writeErr, context.DeadlineExceeded), errors.As(writeErr, &ne) && ne.Timeout():
			sess.closeWith("timeout")
			return rpc.SendResult{}, rpc.NewError(rpc.CodeTimeout)
		default:
			sess.closeWith(reason(writeErr))
			return rpc.SendResult{}, rpc.NewError(rpc.CodeUnavailable)
		}
	}
	if !sess.clipAckCap {
		return rpc.SendResult{State: rpc.SendSent, ReceiptStage: rpc.ReceiptLocalWritten}, nil
	}
	_ = src.ReportProgress(ctx, rpc.SendProgress{
		GrantID:    g.GrantID,
		Stage:      rpc.SendStageAwaitingReceipt,
		BytesSent:  rpc.U64(size),
		TotalBytes: rpc.U64(size),
	})
	ackTimer := time.NewTimer(b.ackBudget())
	defer ackTimer.Stop()
	select {
	case status := <-ackCh:
		switch status {
		case wire.ClipAckReceived:
			return rpc.SendResult{State: rpc.SendSent, ReceiptStage: rpc.ReceiptRemoteReceived}, nil
		case wire.ClipAckDiscarded:
			return rpc.SendResult{State: rpc.SendSent, ReceiptStage: rpc.ReceiptRemoteDiscarded}, nil
		default:
			return rpc.SendResult{}, rpc.NewError(rpc.CodeUnavailable)
		}
	case <-sess.done:
		return rpc.SendResult{}, rpc.NewError(rpc.CodeUnavailable)
	case <-ctx.Done():
		return rpc.SendResult{}, rpc.NewError(rpc.CodeCancelled)
	case <-ackTimer.C:
		return rpc.SendResult{}, rpc.NewError(rpc.CodeTimeout)
	}
}

// ---- receiving ----

func (b *Backend) recvLoop(s *session) error {
	for {
		typ, raw, err := s.w.ReadRaw()
		if err != nil {
			return err
		}
		s.markRx()
		if typ == "ping" {
			s.handlePing(raw)
			continue
		}
		if typ == "pong" {
			s.handlePong(raw)
			continue
		}
		if typ == "clip_ack" {
			s.handleClipAck(raw)
			continue
		}
		if typ == "relay_profile" {
			s.handleRelayProfile(raw)
			continue
		}
		if typ == "relay_handoff_offer" {
			s.handleRelayHandoffOffer(raw)
			continue
		}
		if typ == "relay_handoff_ack" {
			s.handleRelayHandoffAck(raw)
			continue
		}
		if typ == "relay_chunk_data" {
			if err := s.handleRelayChunkData(raw); err != nil {
				return err
			}
			continue
		}
		if typ == "introduce" {
			var msg introduction
			if err := json.Unmarshal(raw, &msg); err != nil {
				return rpc.NewError(rpc.CodeInvalidArgument)
			}
			b.mu.Lock()
			enabled := b.st.Introduce
			b.mu.Unlock()
			req := rpc.Introduce{ConnectionID: s.connID, SessionToken: s.token, Candidates: msg.Candidates}
			if req.Validate() != nil {
				return rpc.NewError(rpc.CodeInvalidArgument)
			}
			if enabled {
				ctx, cancel := context.WithTimeout(s.r.ctx, eventTimeout)
				_ = b.authority().Introduce(ctx, req)
				cancel()
			}
			continue
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
	// Header waits remain unbounded; only a declared body uses the I/O budget.
	s.bodyActive.Store(true)
	defer s.bodyActive.Store(false)
	if s.bodyConn != nil {
		s.bodyConn.body = true
		defer func() { s.bodyConn.body = false; _ = s.conn.SetReadDeadline(time.Time{}) }()
	}
	discardAndAck := func() error {
		if err := s.w.Discard(m); err != nil {
			return err
		}
		s.enqueueAck(wire.ClipAck{T: "clip_ack", ID: m.ID, Status: wire.ClipAckDiscarded})
		return nil
	}
	rcv := b.receiver()
	if rcv == nil {
		return discardAndAck()
	}
	o := rpc.ReceiveOpen{ConnectionID: s.connID, SessionToken: s.token,
		Clip: rpc.Clip{SourceApplication: m.SourceApplication.Clone(), WireID: m.ID, Origin: m.Origin, AtMs: rpc.U64(max(m.TS, 0)), Hash: m.Hash}}
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
		return discardAndAck() // malformed for the Engine: read and drop
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
		return discardAndAck()
	}
	epoch := b.epoch()
	size, kind := clipMeasurement(o.Clip.Parts)
	path := s.startPath(epoch)
	b.emit(syncdiag.Record{Epoch: epoch, Kind: "transfer_size", Direction: "incoming", Declared: size, ContentKind: kind})
	body := &bodyObservation{}
	parts := &recvParts{w: s.w, m: m, observation: body}
	result, recvErr := rcv.Receive(ctx, *g.ReceiveID, parts)
	var drainErr error
	switch {
	case recvErr != nil:
		// An aborted batch must not wait for a peer to finish the rejected body.
		// Returning an error closes the session and releases its readers.
		drainErr = recvErr
	case result == rpc.OutcomeCommit:
		drainErr = parts.drain()
		if drainErr == nil {
			s.enqueueAck(wire.ClipAck{T: "clip_ack", ID: m.ID, Status: wire.ClipAckReceived})
		}
	case result == rpc.OutcomeDiscard:
		drainErr = parts.drain()
		if drainErr == nil {
			s.enqueueAck(wire.ClipAck{T: "clip_ack", ID: m.ID, Status: wire.ClipAckDiscarded})
		}
	default:
		drainErr = rpc.NewError(rpc.CodeUnavailable)
	}
	status := "succeeded"
	if recvErr != nil || result != rpc.OutcomeCommit || drainErr != nil {
		status = "failed"
		if ctx.Err() != nil {
			status = "cancelled"
		}
	}
	b.emit(syncdiag.Record{Epoch: epoch, Kind: "transfer", Direction: "incoming", Result: status, Declared: size, Bytes: body.bytes, Duration: body.duration(), Path: path})
	return drainErr
}

// recvParts serves the bodies of an admitted clip to the Engine in order.
type recvParts struct {
	observation *bodyObservation
	w           *wire.Conn
	m           *wire.Clip
	next        int
	cur         *recvBody
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
	var reader = p.w.Body(size)
	if p.observation != nil {
		reader = measuredReader{r: reader, o: p.observation}
	}
	p.cur = &recvBody{r: reader, left: size}
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
		if _, err := io.CopyN(io.Discard, measuredReader{r: p.w.Body(n), o: p.observation}, n); err != nil {
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
	epoch := b.epoch()
	var abandoned []recoveryObservation
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
	for id, rec := range b.recoveries {
		if p, ok := want[id]; !ok || uint64(p.TrustRevision) != rec.trustRev {
			abandoned = append(abandoned, rec)
			delete(b.recoveries, id)
		}
	}
	genChanged := uint64(st.NetworkGeneration) != b.gen
	b.gen = uint64(st.NetworkGeneration)
	var drop []*session
	var probeLive []*session
	for id, s := range b.sessions {
		if p, ok := want[id]; !ok || uint64(p.TrustRevision) != s.trustRev {
			drop = append(drop, s)
		} else if genChanged && !s.isDone() {
			probeLive = append(probeLive, s)
		}
	}
	now := time.Now()
	for id, d := range b.dialers {
		p, ok := want[id]
		if !ok || p.Address != d.addr || (genChanged && !b.connectedLocked(id)) {
			d.cancel()
			delete(b.dialers, id)
			if !ok || p.Address != d.addr {
				delete(b.dialing, id)
			} else if ds, has := b.dialing[id]; has && ds.owner == d {
				ds.owner = nil
				ds.inFlight = false
				b.dialing[id] = ds
			}
		}
	}
	for id := range b.dialing {
		if _, ok := want[id]; !ok {
			delete(b.dialing, id)
			delete(b.lastChecked, id)
		}
	}
	var start []rpc.StatePeer
	var expiredNotify []string
	for _, p := range st.Peers {
		if p.Address != "" && b.dialers[p.ID] == nil {
			start = append(start, p)
			if !b.connectedLocked(p.ID) {
				ds, existed := b.dialing[p.ID]
				if !existed {
					b.dialing[p.ID] = dialStatus{
						attempts:          0,
						activeUntil:       now.Add(connectingGraceWindow),
						visibleConnecting: true,
					}
				} else if ds.visibleConnecting && !now.Before(ds.activeUntil) {
					ds.visibleConnecting = false
					ds.attempts = max(ds.attempts, maxConnectingAttempts)
					ds.activeUntil = time.Time{}
					b.dialing[p.ID] = ds
					expiredNotify = append(expiredNotify, p.ID)
				}
			}
		}
	}
	b.mu.Unlock()
	for _, id := range expiredNotify {
		b.notifyLinkChanged(id)
	}
	for _, rec := range abandoned {
		if epoch != 0 && rec.epoch == epoch {
			b.emit(syncdiag.Record{Epoch: epoch, Kind: "recovery", Result: "abandoned"})
		}
	}
	for _, s := range drop {
		s.closeWith("revocation")
		<-s.exited // the Engine has been told; nothing of it runs on
	}
	for _, s := range probeLive {
		sess := s
		b.spawn(r, func() { b.probeAndHeal(sess) })
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
			if b.dialOnce(ctx, r, d, p) {
				backoff = time.Second
				continue
			}
			notify := false
			b.mu.Lock()
			if b.dialers[p.ID] == d && !b.connectedLocked(p.ID) {
				now := time.Now()
				ds := b.dialing[p.ID]
				wasVisible := ds.visibleConnecting
				if ds.owner == d {
					ds.owner = nil
					ds.inFlight = false
				}
				ds.attempts++
				if ds.attempts >= maxConnectingAttempts || !now.Before(ds.activeUntil) {
					ds.attempts = max(ds.attempts, maxConnectingAttempts)
					ds.activeUntil = time.Time{}
					ds.visibleConnecting = false
				}
				b.dialing[p.ID] = ds
				notify = wasVisible && !ds.visibleConnecting
			}
			b.mu.Unlock()
			if notify {
				b.notifyLinkChanged(p.ID)
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
func (b *Backend) dialOnce(ctx context.Context, r *runState, d *dialer, p rpc.StatePeer) bool {
	if err := b.waitTransport(ctx); err != nil {
		return false
	}
	b.mu.Lock()
	if b.dialers[p.ID] != d {
		b.mu.Unlock()
		return false
	}
	ds := b.dialing[p.ID]
	ds.owner = d
	ds.inFlight = true
	b.dialing[p.ID] = ds
	b.mu.Unlock()
	defer func() {
		b.mu.Lock()
		if cur, ok := b.dialing[p.ID]; ok && cur.owner == d {
			cur.inFlight = false
			cur.owner = nil
			b.dialing[p.ID] = cur
		}
		b.mu.Unlock()
	}()
	dctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	conn, err := b.dial(dctx, p.Address)
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
