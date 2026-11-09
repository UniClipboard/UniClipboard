package engine

import (
	"bytes"
	"context"
	"crypto/subtle"
	"encoding/base64"
	"errors"
	"time"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// The identity handshake of an out-of-process peer_sessions backend
// (sync-backend-contract §3, identity-protocol §4). The Engine keeps the device
// key, the Hello nonces and the transcript; the backend only carries bytes. No
// state survives a handshake that failed, finished or timed out.
const (
	handshakeTTL   = 60 * time.Second
	maxHandshakes  = 8
	attemptTTL     = 5 * time.Minute
	maxAttempts    = 16
	maxSessions    = 1024
	nonceMemory    = 10 * time.Minute
	maxNonceMemory = 4096
)

var b64 = base64.RawURLEncoding

// attempt is an explicit outgoing connection the Engine started: it is bound
// to one address, optionally to the peer it must prove to be, and to the
// pairing token it proves possession of. A backend cannot make one up.
type attempt struct {
	id       string
	address  string // secret
	expected string // required peer ID, or ""
	tok      []byte // pairing token, never leaves the Engine
	expires  time.Time
	conn     string // the connection that took it
}

type handshake struct {
	conn    string
	dir     string
	attempt *attempt
	binding []byte
	mine    identity.Hello
	mineEnc []byte
	peer    identity.Hello
	t       [32]byte
	proved  bool
	started time.Time
}

// session is a connection the Engine allowed. Step 3 validates connection
// events against it.
type session struct {
	conn      string
	peerID    string
	token     string
	outgoing  bool
	trustRev  uint64
	gen       uint64
	seq       uint64 // highest event sequence seen
	state     string // state reported at seq
	connected bool
	ended     time.Time // set once disconnected, replaced or revoked
}

const endedMemory = 2 * time.Minute

// newAttempt registers an explicit attempt and returns its ID.
func (b *peerProcBackend) newAttempt(address, expected string, tok []byte) (string, error) {
	b.am.Lock()
	defer b.am.Unlock()
	now := time.Now()
	b.sweepLocked(now)
	if len(b.att) >= maxAttempts {
		return "", rpc.NewError(rpc.CodeExhausted)
	}
	a := &attempt{id: randHex(32), address: address, expected: expected, tok: bytes.Clone(tok), expires: now.Add(attemptTTL)}
	if b.att == nil {
		b.att = map[string]*attempt{}
	}
	b.att[a.id] = a
	return a.id, nil
}

// endAttempt ends the attempt; it can grant nothing afterwards.
func (b *peerProcBackend) endAttempt(id string) {
	b.am.Lock()
	delete(b.att, id)
	b.am.Unlock()
}

func (b *peerProcBackend) sweepLocked(now time.Time) {
	for k, h := range b.hs {
		if now.Sub(h.started) > handshakeTTL {
			delete(b.hs, k)
		}
	}
	for k, a := range b.att {
		if now.After(a.expires) {
			delete(b.att, k)
		}
	}
	for k, s := range b.sess {
		if !s.ended.IsZero() && now.Sub(s.ended) > endedMemory {
			delete(b.sess, k)
		}
	}
	for k, at := range b.nonces {
		if now.Sub(at) > nonceMemory {
			delete(b.nonces, k)
		}
	}
}

func rpcErr(code string) error { return rpc.NewError(code) }

// IdentityHello starts the handshake of one connection.
func (b *peerProcBackend) IdentityHello(_ context.Context, r rpc.IdentityHello) (rpc.IdentityHelloResult, error) {
	e := b.e
	binding, derr := b64.DecodeString(r.ContextBinding)
	if derr != nil {
		return rpc.IdentityHelloResult{}, rpcErr(rpc.CodeInvalidArgument)
	}
	b.am.Lock()
	defer b.am.Unlock()
	now := time.Now()
	b.sweepLocked(now)
	if b.hs[r.ConnectionID] != nil || b.sess[r.ConnectionID] != nil {
		return rpc.IdentityHelloResult{}, rpcErr(rpc.CodeConflict)
	}
	if len(b.hs) >= maxHandshakes {
		return rpc.IdentityHelloResult{}, rpcErr(rpc.CodeExhausted)
	}
	var att *attempt
	if r.AttemptID != nil {
		att = b.att[*r.AttemptID]
		switch {
		case att == nil:
			return rpc.IdentityHelloResult{}, rpcErr(rpc.CodeNotFound)
		case r.Direction != "outgoing":
			return rpc.IdentityHelloResult{}, rpcErr(rpc.CodeInvalidArgument)
		case att.conn != "":
			return rpc.IdentityHelloResult{}, rpcErr(rpc.CodeConflict)
		}
		att.conn = r.ConnectionID
	}
	hello, err := identity.NewHello(e.ident.Public())
	if err != nil {
		return rpc.IdentityHelloResult{}, rpcErr(rpc.CodeInternal)
	}
	enc := hello.Encode()
	if b.hs == nil {
		b.hs = map[string]*handshake{}
	}
	b.hs[r.ConnectionID] = &handshake{conn: r.ConnectionID, dir: r.Direction, attempt: att, binding: binding, mine: hello, mineEnc: enc, started: now}
	return rpc.IdentityHelloResult{Hello: b64.EncodeToString(enc)}, nil
}

// fail drops the handshake and frees its attempt.
func (b *peerProcBackend) failLocked(h *handshake, code string) error {
	delete(b.hs, h.conn)
	if h.attempt != nil {
		delete(b.att, h.attempt.id) // an attempt serves one connection
	}
	return rpcErr(code)
}

func (b *peerProcBackend) live(conn string, now time.Time) (*handshake, error) {
	b.sweepLocked(now)
	h := b.hs[conn]
	if h == nil {
		return nil, rpcErr(rpc.CodeNotFound)
	}
	return h, nil
}

// IdentityProve takes the peer's Hello and signs this connection's transcript.
func (b *peerProcBackend) IdentityProve(_ context.Context, r rpc.IdentityProve) (rpc.IdentityProveResult, error) {
	raw, derr := b64.DecodeString(r.PeerHello)
	if derr != nil {
		return rpc.IdentityProveResult{}, rpcErr(rpc.CodeInvalidArgument)
	}
	b.am.Lock()
	defer b.am.Unlock()
	now := time.Now()
	h, err := b.live(r.ConnectionID, now)
	if err != nil {
		return rpc.IdentityProveResult{}, err
	}
	if h.proved {
		return rpc.IdentityProveResult{}, rpcErr(rpc.CodeConflict)
	}
	peer, perr := identity.ParseHello(raw)
	if perr != nil {
		return rpc.IdentityProveResult{}, b.failLocked(h, rpc.CodeInvalidArgument)
	}
	if _, verr := identity.SelectVersion(h.mine, peer); verr != nil {
		return rpc.IdentityProveResult{}, b.failLocked(h, "protocol_mismatch")
	}
	if bytes.Equal(peer.PK, b.e.ident.Public()) {
		return rpc.IdentityProveResult{}, b.failLocked(h, "identity_mismatch")
	}
	key := string(peer.Nonce)
	if _, seen := b.nonces[key]; seen || bytes.Equal(peer.Nonce, h.mine.Nonce) {
		return rpc.IdentityProveResult{}, b.failLocked(h, rpc.CodeReplay)
	}
	if h.attempt != nil && h.attempt.expected != "" && identity.DeviceID(peer.PK) != h.attempt.expected {
		return rpc.IdentityProveResult{}, b.failLocked(h, "identity_mismatch")
	}
	if len(b.nonces) >= maxNonceMemory {
		return rpc.IdentityProveResult{}, b.failLocked(h, rpc.CodeExhausted)
	}
	helloI, helloR, role := h.mineEnc, raw, identity.RoleInitiator
	if h.dir == "incoming" {
		helloI, helloR, role = raw, h.mineEnc, identity.RoleResponder
	}
	t, terr := identity.Transcript(identity.KindPeerSessions, h.binding, helloI, helloR)
	if terr != nil {
		return rpc.IdentityProveResult{}, b.failLocked(h, rpc.CodeInvalidArgument)
	}
	if b.nonces == nil {
		b.nonces = map[string]time.Time{}
	}
	b.nonces[key] = now
	h.peer, h.t, h.proved = peer, t, true
	out := rpc.IdentityProveResult{Proof: b64.EncodeToString(b.e.ident.Prove(role, t))}
	if h.dir == "outgoing" && h.attempt != nil && len(h.attempt.tok) > 0 {
		pp := b64.EncodeToString(identity.PairProof(h.attempt.tok, t))
		out.PairProof = &pp
	}
	return out, nil
}

func sameAttempt(a *attempt, id *string) bool {
	if a == nil || id == nil {
		return a == nil && id == nil
	}
	return subtle.ConstantTimeCompare([]byte(a.id), []byte(*id)) == 1
}

// CheckPeer verifies the peer's proof and decides. The handshake ends here,
// whatever the outcome.
func (b *peerProcBackend) CheckPeer(_ context.Context, r rpc.CheckPeer) (rpc.PeerDecision, error) {
	e := b.e
	proof, e1 := b64.DecodeString(r.PeerProof)
	var pair []byte
	var e2 error
	if r.PeerPairProof != nil {
		pair, e2 = b64.DecodeString(*r.PeerPairProof)
	}
	if e1 != nil || e2 != nil {
		return rpc.PeerDecision{}, rpcErr(rpc.CodeInvalidArgument)
	}
	b.am.Lock()
	h, err := b.live(r.ConnectionID, time.Now())
	if err != nil {
		b.am.Unlock()
		return rpc.PeerDecision{}, err
	}
	if !h.proved {
		b.am.Unlock()
		return rpc.PeerDecision{}, rpcErr(rpc.CodeInvalidState)
	}
	delete(b.hs, h.conn) // single use
	if h.attempt != nil {
		delete(b.att, h.attempt.id)
	}
	b.am.Unlock()
	if r.Direction != h.dir || !sameAttempt(h.attempt, r.AttemptID) {
		return rpc.PeerDecision{}, rpcErr(rpc.CodeBindingMismatch)
	}
	peerRole := identity.RoleResponder
	if h.dir == "incoming" {
		peerRole = identity.RoleInitiator
	}
	if identity.VerifyProof(h.peer.PK, peerRole, h.t, proof) != nil {
		return rpc.PeerDecision{}, rpcErr(rpc.CodeBadProof)
	}
	ticket := false
	if pair != nil {
		// Only the initiator proves a ticket, and only a live local one counts.
		if h.dir != "incoming" || len(pair) != rpc.PairProofBytes || !e.pairUsedBy(b.id, h.t, pair) {
			return rpc.PeerDecision{}, rpcErr(rpc.CodeBadPairProof)
		}
		ticket = true
	}
	id := identity.DeviceID(h.peer.PK)
	prev, perr := e.st.Device(id)
	wasRevoked := perr == nil && prev.Trust == store.TrustRevoked
	v := peerbackend.Verified{PublicKey: h.peer.PK, Name: r.Hello.Name, Addr: r.Hello.Address, Context: []byte{}, Ticket: ticket}
	d, bd, rerr := e.recordPeer(string(b.id), v)
	switch {
	case errors.Is(rerr, errTooManyPending):
		return rpc.PeerDecision{PeerID: id, Decision: "reject"}, nil
	case errors.Is(rerr, store.ErrIdentityConflict):
		return rpc.PeerDecision{}, rpcErr("identity_mismatch")
	case rerr != nil:
		return rpc.PeerDecision{}, rpcErr(rpc.CodeInternal)
	}
	if wasRevoked {
		e.syncPeerStates() // the tombstone is gone from the snapshot
	}
	rev := uint64(d.Revision + bd.Revision)
	if d.Trust != store.TrustTrusted || bd.State != store.BindingAuthorized {
		return rpc.PeerDecision{PeerID: id, Decision: "pending", TrustRevision: rpc.U64(rev)}, nil
	}
	tok := randHex(32)
	e.mu.Lock() // never take Engine.mu while holding am: state queries do the reverse
	gen := e.netGen
	e.mu.Unlock()
	b.am.Lock()
	defer b.am.Unlock()
	if len(b.sess) >= maxSessions {
		return rpc.PeerDecision{}, rpcErr(rpc.CodeExhausted)
	}
	if b.sess == nil {
		b.sess = map[string]*session{}
	}
	b.sess[h.conn] = &session{conn: h.conn, peerID: id, token: tok, outgoing: h.dir == "outgoing", trustRev: rev, gen: gen}
	return rpc.PeerDecision{PeerID: id, Decision: "allow", SessionToken: &tok, TrustRevision: rpc.U64(rev)}, nil
}

// trustRevision is the revision the snapshot reports for a peer, and ok only
// while the device may still exchange content on this instance.
func (b *peerProcBackend) trustRevision(peerID string) (uint64, bool) {
	d, err := b.e.st.Device(peerID)
	if err != nil || d.Trust != store.TrustTrusted {
		return 0, false
	}
	bd, err := b.e.st.InstanceBinding(peerID, string(b.id))
	if err != nil || bd.State != store.BindingAuthorized {
		return 0, false
	}
	return uint64(d.Revision + bd.Revision), true
}

// syncEnabled reports whether the user has not paused sync on this path.
func (b *peerProcBackend) syncEnabled(peerID string) bool {
	bd, err := b.e.st.InstanceBinding(peerID, string(b.id))
	return err == nil && bd.SyncEnabled
}

// dialer is the device that opened the connection; of two simultaneous
// connections to one peer the one dialed by the smaller device ID survives,
// the rule the tailcat session layer uses (both ends agree without talking).
func (b *peerProcBackend) dialer(s *session) string {
	if s.outgoing {
		return b.e.ident.DeviceID()
	}
	return s.peerID
}

// PeerEvent applies a connection report. The token must be the one check_peer
// issued for exactly this connection; a stale sequence is ignored, a repeated
// one is idempotent, and a different report for the same sequence conflicts.
func (b *peerProcBackend) PeerEvent(_ context.Context, r rpc.PeerEvent) error {
	b.e.mu.Lock() // before am: state queries take Engine.mu and then am
	gen := b.e.netGen
	b.e.mu.Unlock()
	b.am.Lock()
	b.sweepLocked(time.Now())
	s := b.sess[r.ConnectionID]
	if s == nil {
		b.am.Unlock()
		return rpcErr(rpc.CodeNotFound)
	}
	if subtle.ConstantTimeCompare([]byte(s.token), []byte(r.SessionToken)) != 1 {
		b.am.Unlock()
		return rpcErr(rpc.CodePermission)
	}
	seq := uint64(r.Sequence)
	switch {
	case seq < s.seq:
		b.am.Unlock()
		return nil
	case seq == s.seq:
		b.am.Unlock()
		if r.State != s.state {
			return rpcErr(rpc.CodeConflict)
		}
		return nil
	}
	changed := false
	if r.State == "disconnected" {
		s.seq, s.state = seq, r.State
		if s.ended.IsZero() {
			s.ended = time.Now()
		}
		changed = s.connected
		s.connected = false
	} else {
		// A token that ended can never connect again; a new connection must
		// pass check_peer first. Revoked authority or a newer network
		// generation end it too.
		rev, ok := b.trustRevision(s.peerID)
		switch {
		case !s.ended.IsZero():
			b.am.Unlock()
			return rpcErr(rpc.CodeInvalidState)
		case !ok || rev != s.trustRev:
			s.ended = time.Now()
			b.am.Unlock()
			return rpcErr(rpc.CodePermission)
		case gen != s.gen:
			s.ended = time.Now()
			b.am.Unlock()
			return rpcErr(rpc.CodeInvalidState)
		}
		for _, o := range b.sess {
			if o == s || o.peerID != s.peerID || !o.connected {
				continue
			}
			if b.dialer(o) <= b.dialer(s) {
				s.ended = time.Now()
				b.am.Unlock()
				return rpcErr(rpc.CodeConflict) // the other connection wins
			}
			o.connected, o.ended = false, time.Now() // replaced; its reports no longer matter
		}
		s.seq, s.state, changed, s.connected = seq, r.State, !s.connected, true
	}
	peer := s.peerID
	b.am.Unlock()
	if changed {
		if r.State == "connected" {
			if bd, err := b.e.st.InstanceBinding(peer, string(b.id)); err == nil {
				_ = b.e.st.ReportBinding(bd.ID, time.Now().UnixMilli(), "")
			}
		}
		b.e.deviceChanged(peer)
	}
	return nil
}

// connected reports whether a live connection to the device is up.
func (b *peerProcBackend) connected(peerID string) bool {
	b.am.Lock()
	defer b.am.Unlock()
	for _, s := range b.sess {
		if s.peerID == peerID && s.connected {
			return true
		}
	}
	return false
}

// pruneSessions ends sessions of peers that no longer hold the authority the
// session was granted under. It runs inside the state barrier.
func (b *peerProcBackend) pruneSessions(st rpc.SyncState) {
	live := make(map[string]uint64, len(st.Peers))
	for _, p := range st.Peers {
		live[p.ID] = uint64(p.TrustRevision)
	}
	var gone []string
	b.am.Lock()
	for _, s := range b.sess {
		if rev, ok := live[s.peerID]; (!ok || rev != s.trustRev) && s.ended.IsZero() {
			s.ended = time.Now()
			if s.connected {
				s.connected = false
				gone = append(gone, s.peerID)
			}
		}
	}
	b.am.Unlock()
	for _, id := range gone {
		b.e.deviceChanged(id)
	}
}

// ProcessEnded ends every session and pending handshake: the backend process
// that held the connections is gone and can no longer report them. The
// restarted process starts with none, and must pass check_peer again.
func (b *peerProcBackend) ProcessEnded() {
	var gone []string
	b.am.Lock()
	now := time.Now()
	for _, s := range b.sess {
		if s.ended.IsZero() {
			s.ended = now
		}
		if s.connected {
			s.connected = false
			gone = append(gone, s.peerID)
		}
	}
	clear(b.hs)
	b.am.Unlock()
	for _, id := range gone {
		b.e.deviceChanged(id)
	}
}
