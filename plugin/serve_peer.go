package plugin

import (
	"bytes"
	"context"
	"io"
	"sync/atomic"
	"time"

	"github.com/uniclipboard/unic/plugin/rpc"
)

// PeerBackend is implemented by peer_sessions sync backends. The host binds
// one session, then delivers whole-state snapshots. ApplyState must return
// only after the backend has stopped using anything the new state revokes
// (closed sessions to removed peers, dropped queued sends): its return is the
// revocation barrier the host waits on.
type PeerBackend interface {
	Bind(ctx context.Context, b rpc.Bind) error
	ApplyState(ctx context.Context, s rpc.SyncState) error
}

// PeerLinkInspector is an optional capability implemented by peer_sessions
// backends that can expose sanitized per-peer link state (connecting, path,
// latency) and execute explicit bounded connection probes.
type PeerLinkInspector interface {
	LinkSnapshots() map[string]rpc.PeerLinkStatus
	ProbePeers(ctx context.Context, peerIDs []string) ([]rpc.PeerLinkStatus, error)
}

// PeerLinkCallbackAware is an optional in-process hook allowing a peer backend
// to notify the Engine when a peer's non-session link state changes (e.g.
// entering or leaving connecting state, or completing a wake warmup probe).
type PeerLinkCallbackAware interface {
	SetLinkStatusChanged(fn func(peerID string))
}

// RelayProfile describes one peer's E2EE HTTP relay mailbox route and revocable
// delivery token exchanged over an authenticated peer session (0036).
type RelayProfile struct {
	RelayOrigin   string
	MailboxID     string
	DeliveryToken string
	MaxBytes      int64
	Epoch         uint64
}

// PeerRelayBridge is an optional in-process hook allowing built-in peer_sessions
// backends to exchange relay_profile control messages and accept non-blocking
// dial wakeup hints from Engine's hybrid relay coordinator.
type PeerRelayBridge interface {
	SetRelayCallbacks(
		provide func(peerID string) (RelayProfile, bool),
		onPeerProfile func(peerID string, p RelayProfile),
	)
	PushRelayProfiles()
	WakePeer(peerID string)
}

// RelayHandoffOffer proposes settling an in-flight chunked relay transfer over
// a recovered P2P session (0036 Phase 2).
type RelayHandoffOffer struct {
	TransferID          string
	EnvelopeJSON        []byte
	RelayUploadedBitmap string
}

// RelayHandoffAck reports the receiver's verified chunk bitmap and the chunks
// it wants pushed over P2P.
type RelayHandoffAck struct {
	TransferID    string
	Accept        bool
	Reason        string
	HaveBitmap    string
	WantP2PBitmap string
}

// PeerRelayHandoffBridge is an optional in-process hook allowing built-in
// peer_sessions backends to carry Level 3 chunked handoff control/data frames
// and clip_ack coordination for Engine's hybrid relay coordinator (0036 Phase 2).
type PeerRelayHandoffBridge interface {
	SetRelayHandoffCallbacks(
		onOffer func(peerID string, offer RelayHandoffOffer) RelayHandoffAck,
		onChunk func(peerID, transferID string, index int, ciphertext []byte),
		onPeerConnected func(peerID string, handoffCap, clipAckCap bool),
		onPeerDisconnected func(peerID string),
	)
	PeerHandoffCaps(peerID string) (connected, handoffCap, clipAckCap bool)
	RegisterRelayClipAckWaiter(peerID, transferID string) (ackCh <-chan string, sessDone <-chan struct{}, cleanup func(), ok bool)
	SendRelayHandoffOffer(ctx context.Context, peerID string, offer RelayHandoffOffer) (RelayHandoffAck, error)
	SendRelayChunk(ctx context.Context, peerID, transferID string, index int, ciphertext []byte) error
	SendRelayClipAck(peerID, transferID, status string) bool
}

type peerState struct {
	bound bool
	bind  rpc.Bind
	rev   uint64
	cur   rpc.SyncState
}

func (s *server) bindPeer(ctx context.Context, c *rpc.Call) (any, error) {
	pb, ok := s.p.(PeerBackend)
	if !ok {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.Bind
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.initialized(); err != nil {
		return nil, err
	}
	s.mu.Lock()
	if s.peer.bound {
		same := s.peer.bind == in
		s.mu.Unlock()
		if same {
			return nil, nil
		}
		return nil, rpc.NewError(rpc.CodeConflict)
	}
	s.mu.Unlock()
	if err := safely(func() error { return pb.Bind(ctx, in) }); err != nil {
		return nil, err
	}
	s.mu.Lock()
	s.peer.bound, s.peer.bind = true, in
	s.mu.Unlock()
	return nil, nil
}

func (s *server) applyState(ctx context.Context, c *rpc.Call) (any, error) {
	pb, ok := s.p.(PeerBackend)
	if !ok {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.ApplyState
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.initialized(); err != nil {
		return nil, err
	}
	rs, err := c.Conn.Expect(ctx, c.ID, in.Snapshot, rpc.MaxStateBytes)
	if err != nil {
		return nil, err
	}
	var buf bytes.Buffer
	_, err = io.Copy(&buf, rs)
	_ = rs.Close()
	if err != nil {
		return nil, err
	}
	var st rpc.SyncState
	if err := rpc.Decode(buf.Bytes(), &st); err != nil || st.Validate() != nil || st.Revision != in.Revision {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	s.mu.Lock()
	if !s.peer.bound || st.BindingID != s.peer.bind.BindingID || st.BackendID != s.peer.bind.BackendID {
		s.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	cur := s.peer
	s.mu.Unlock()
	switch {
	case uint64(st.Revision) < cur.rev:
		return nil, rpc.NewError(rpc.CodeConflict)
	case uint64(st.Revision) == cur.rev:
		if !st.Equal(cur.cur) {
			return nil, rpc.NewError(rpc.CodeConflict)
		}
		return rpc.Revision{Revision: st.Revision}, nil
	}
	if err := safely(func() error { return pb.ApplyState(ctx, st) }); err != nil {
		return nil, err
	}
	s.mu.Lock()
	s.peer.rev, s.peer.cur = uint64(st.Revision), st
	s.mu.Unlock()
	return rpc.Revision{Revision: st.Revision}, nil
}

// PeerSender is implemented by peer_sessions backends that send clips. Send
// writes one clip to one connected peer. It must call h.Authorize before the
// first wire byte, then pull each part with h.OpenPart. Returning a zero
// SendResult defaults to state="sent" and receipt_stage="local_written".
type PeerSender interface {
	Send(ctx context.Context, s rpc.Send, h SendHost) (rpc.SendResult, error)
}

// SendHost is the backend's view of one backend.send call. It dies with it.
type SendHost interface {
	Authorize(ctx context.Context) (rpc.SendGrant, error)
	// OpenPart returns the bytes of part index under the grant; the caller
	// closes it. Each part opens once.
	OpenPart(ctx context.Context, grantID string, index int) (io.ReadCloser, error)
	// ReportProgress optionally reports throttled byte/stage progress under the grant.
	ReportProgress(ctx context.Context, p rpc.SendProgress) error
}

type remoteSendHost struct {
	conn *rpc.Conn
	root uint64
	in   rpc.Send
}

func (h remoteSendHost) Authorize(ctx context.Context) (rpc.SendGrant, error) {
	var g rpc.SendGrant
	_, err := h.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodAuthorizeSend, Parent: h.root, Timeout: 30 * time.Second,
		Params: rpc.Authorize{DispatchID: h.in.DispatchID, ContentHandle: h.in.ContentHandle, ConnectionID: h.in.ConnectionID, SessionToken: h.in.SessionToken}}, &g)
	if err != nil {
		return g, err
	}
	if g.Validate() != nil {
		return rpc.SendGrant{}, rpc.NewError(rpc.CodeInternal)
	}
	return g, nil
}

func (h remoteSendHost) OpenPart(ctx context.Context, grantID string, index int) (io.ReadCloser, error) {
	if index < 0 || index >= len(h.in.Clip.Parts) {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	var d rpc.StreamDesc
	_, err := h.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodOpenSendPart, Parent: h.root, Timeout: 30 * time.Second,
		Params: rpc.OpenSendPart{GrantID: grantID, Index: index}}, &d)
	if err != nil {
		return nil, err
	}
	if d.Purpose != rpc.PurposeSyncPart || uint64(d.Size) != uint64(h.in.Clip.Parts[index].Size) {
		return nil, rpc.NewError(rpc.CodeInternal)
	}
	if d.Size == 0 {
		if d.StreamID != 0 {
			return nil, rpc.NewError(rpc.CodeInternal)
		}
		return io.NopCloser(bytes.NewReader(nil)), nil // empty parts have no stream
	}
	return h.conn.Expect(ctx, h.root, d, uint64(h.in.Clip.Parts[index].Size))
}

func (h remoteSendHost) ReportProgress(ctx context.Context, p rpc.SendProgress) error {
	if err := p.Validate(); err != nil {
		return err
	}
	_, err := h.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodSendProgress, Parent: h.root, Timeout: 5 * time.Second, Params: p}, nil)
	return err
}

func (s *server) send(ctx context.Context, c *rpc.Call) (any, error) {
	ps, ok := s.p.(PeerSender)
	if !ok {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.Send
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.started(); err != nil {
		return nil, err
	}
	s.mu.Lock()
	ok = s.peer.bound && in.BackendID == s.peer.bind.BackendID
	s.mu.Unlock()
	if !ok {
		return nil, rpc.NewError(rpc.CodeInvalidState)
	}
	var res rpc.SendResult
	if err := safely(func() (err error) {
		res, err = ps.Send(ctx, in, remoteSendHost{conn: c.Conn, root: c.ID, in: in})
		return err
	}); err != nil {
		return nil, err
	}
	if res.State == "" {
		res.State = rpc.SendSent
	}
	if err := res.Validate(); err != nil {
		return nil, rpc.NewError(rpc.CodeInternal)
	}
	return res, nil
}

// SendSource is the Engine side of one send: it authorizes the transfer,
// supplies part bytes, and accepts optional throttled progress updates. The
// host Send validates every request against the backend.send it belongs to
// before calling it.
type SendSource interface {
	Authorize(ctx context.Context, a rpc.Authorize) (rpc.SendGrant, error)
	OpenPart(ctx context.Context, grantID string, index int) (io.ReadCloser, error)
	ReportProgress(ctx context.Context, p rpc.SendProgress) error
}

// PeerDispatcher is the Engine-facing form of PeerSender: it starts one
// backend.send and serves the backend's authorize/open/progress requests from src.
type PeerDispatcher interface {
	Send(ctx context.Context, s rpc.Send, src SendSource) (rpc.SendResult, error)
}

// ReceiveBody is one part's network body; Size is the declared length.
type ReceiveBody struct {
	Size uint64
	R    io.Reader
}

// PeerHost is what a peer_sessions backend asks of the Engine about inbound
// clips: ReceiveOpen before reading any body byte, then Receive with the
// bodies in order. Discard from ReceiveOpen means drain Size bytes per part
// and keep the connection; an error means the header is not drainable.
type PeerHost interface {
	ReceiveOpen(ctx context.Context, o rpc.ReceiveOpen) (rpc.ReceiveGrant, error)
	Receive(ctx context.Context, receiveID string, parts []ReceiveBody) (outcome string, err error)
	// IdentityHello, IdentityProve and CheckPeer are the handshake of
	// sync-backend-contract §3. The Engine verifies; the backend carries bytes.
	IdentityHello(ctx context.Context, r rpc.IdentityHello) (rpc.IdentityHelloResult, error)
	IdentityProve(ctx context.Context, r rpc.IdentityProve) (rpc.IdentityProveResult, error)
	CheckPeer(ctx context.Context, r rpc.CheckPeer) (rpc.PeerDecision, error)
	// PeerEvent reports a connection the peer also accepted going up or down.
	PeerEvent(ctx context.Context, r rpc.PeerEvent) error
	// Introduce passes on the devices a connected peer vouched for. They are
	// suggestions only.
	Introduce(ctx context.Context, r rpc.Introduce) error
}

// PeerHostAware backends get their PeerHost after Init and before Start.
type PeerHostAware interface{ SetPeerHost(PeerHost) }

func (s *server) wirePeer() {
	if a, ok := s.p.(PeerHostAware); ok && s.spec.TransportKind == TransportPeerSessions && !s.spec.ExternalChannels {
		a.SetPeerHost(remotePeerHost{s})
	}
}

type remotePeerHost struct{ s *server }

const receiveCallTimeout = 10 * time.Minute

func (h remotePeerHost) ReceiveOpen(ctx context.Context, o rpc.ReceiveOpen) (rpc.ReceiveGrant, error) {
	if err := o.Validate(); err != nil {
		return rpc.ReceiveGrant{}, err
	}
	var g rpc.ReceiveGrant
	if _, err := h.s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodReceiveOpen, Timeout: 30 * time.Second, Params: o}, &g); err != nil {
		return g, err
	}
	if g.Validate() != nil {
		return rpc.ReceiveGrant{}, rpc.NewError(rpc.CodeInternal)
	}
	return g, nil
}

const identityCallTimeout = 30 * time.Second

func (h remotePeerHost) IdentityHello(ctx context.Context, r rpc.IdentityHello) (rpc.IdentityHelloResult, error) {
	var out rpc.IdentityHelloResult
	if err := r.Validate(); err != nil {
		return out, err
	}
	if _, err := h.s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodIdentityHello, Timeout: identityCallTimeout, Params: r}, &out); err != nil {
		return out, err
	}
	if out.Validate() != nil {
		return rpc.IdentityHelloResult{}, rpc.NewError(rpc.CodeInternal)
	}
	return out, nil
}

func (h remotePeerHost) IdentityProve(ctx context.Context, r rpc.IdentityProve) (rpc.IdentityProveResult, error) {
	var out rpc.IdentityProveResult
	if err := r.Validate(); err != nil {
		return out, err
	}
	if _, err := h.s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodIdentityProve, Timeout: identityCallTimeout, Params: r}, &out); err != nil {
		return out, err
	}
	if out.Validate() != nil {
		return rpc.IdentityProveResult{}, rpc.NewError(rpc.CodeInternal)
	}
	return out, nil
}

func (h remotePeerHost) CheckPeer(ctx context.Context, r rpc.CheckPeer) (rpc.PeerDecision, error) {
	var out rpc.PeerDecision
	if err := r.Validate(); err != nil {
		return out, err
	}
	if _, err := h.s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodCheckPeer, Timeout: identityCallTimeout, Params: r}, &out); err != nil {
		return out, err
	}
	if out.Validate() != nil {
		return rpc.PeerDecision{}, rpc.NewError(rpc.CodeInternal)
	}
	return out, nil
}

func (h remotePeerHost) PeerEvent(ctx context.Context, r rpc.PeerEvent) error {
	if err := r.Validate(); err != nil {
		return err
	}
	_, err := h.s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodPeerEvent, Timeout: identityCallTimeout, Params: r}, nil)
	return err
}

func (h remotePeerHost) Introduce(ctx context.Context, r rpc.Introduce) error {
	if err := r.Validate(); err != nil {
		return err
	}
	_, err := h.s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodIntroduce, Timeout: identityCallTimeout, Params: r}, nil)
	return err
}

func (h remotePeerHost) Receive(ctx context.Context, receiveID string, parts []ReceiveBody) (string, error) {
	conn := h.s.conn
	cctx, cancel := context.WithCancel(ctx)
	defer cancel()
	var feedErr atomic.Pointer[error]
	// The part declarations are children of the receive call, so they start
	// only after its request frame is on the wire.
	feed := func(root uint64) {
		go func() {
			fail := func(err error) { feedErr.Store(&err); cancel() }
			for i, p := range parts {
				d := rpc.StreamDesc{Purpose: rpc.PurposeReceivePart}
				var ss *rpc.SendStream
				if p.Size > 0 {
					var err error
					if ss, d, err = conn.NewSend(conn.Context(), root, rpc.PurposeReceivePart, p.Size); err != nil {
						fail(err)
						return
					}
				}
				if _, err := conn.Call(cctx, rpc.CallOptions{Method: rpc.MethodPeerReceivePart, Parent: root, Timeout: 30 * time.Second,
					Params: rpc.PeerReceivePart{ReceiveID: receiveID, Index: i, Stream: d}}, nil); err != nil {
					fail(err)
					return
				}
				if ss == nil {
					continue
				}
				if _, err := io.Copy(ss, io.LimitReader(p.R, int64(p.Size))); err != nil {
					fail(err)
					return
				}
				if err := ss.Close(); err != nil {
					fail(err)
					return
				}
			}
		}()
	}
	var res rpc.PeerReceiveResult
	id, err := conn.Call(cctx, rpc.CallOptions{Method: rpc.MethodPeerReceive, Timeout: receiveCallTimeout,
		Params: rpc.PeerReceive{ReceiveID: receiveID}, Sent: feed}, &res)
	cancel() // stop the feeder before its streams are released
	conn.ReleaseRoot(id)
	if err != nil {
		if fe := feedErr.Load(); fe != nil {
			return "", *fe
		}
		return "", err
	}
	if res.Validate() != nil {
		return "", rpc.NewError(rpc.CodeInternal)
	}
	return res.Outcome, nil
}

// ReceiveParts hands the Engine each admitted part's body, in order.
type ReceiveParts interface {
	// Next blocks until the backend declares the next part, and returns
	// io.EOF after the last. The caller reads and closes the body before Next.
	Next(ctx context.Context) (index int, body io.ReadCloser, err error)
}

// PeerReceiver is the Engine side of inbound clips. OpenReceive decides on
// the header alone; Receive consumes the granted id, reads every part, and
// returns after processing and commit.
type PeerReceiver interface {
	OpenReceive(ctx context.Context, o rpc.ReceiveOpen) (rpc.ReceiveGrant, error)
	Receive(ctx context.Context, receiveID string, parts ReceiveParts) (outcome string, err error)
}

// PeerAuthority is the Engine side of the identity handshake.
type PeerAuthority interface {
	IdentityHello(ctx context.Context, r rpc.IdentityHello) (rpc.IdentityHelloResult, error)
	IdentityProve(ctx context.Context, r rpc.IdentityProve) (rpc.IdentityProveResult, error)
	CheckPeer(ctx context.Context, r rpc.CheckPeer) (rpc.PeerDecision, error)
	PeerEvent(ctx context.Context, r rpc.PeerEvent) error
	Introduce(ctx context.Context, r rpc.Introduce) error
}

// ProcessEndAware is optionally implemented by a PeerAuthority. The proxy
// calls ProcessEnded when the backend process ended unexpectedly: every
// connection it held is gone, and no event will ever report that.
type ProcessEndAware interface{ ProcessEnded() }

// PeerAuthorityAware is implemented by the host-side proxy of a peer backend.
type PeerAuthorityAware interface{ SetPeerAuthority(PeerAuthority) }

// PeerReceiverAware is implemented by the host-side proxy of a peer backend.
type PeerReceiverAware interface{ SetPeerReceiver(PeerReceiver) }

// PeerPairing is implemented by backends that can take part in pairing: they
// say where they can be reached, and dial an address on the Engine's behalf.
// Connect returns after the handshake, which the Engine decided through
// PeerAuthority; it never grants trust.
type PeerPairing interface {
	PairingAddress(ctx context.Context, purpose string) (string, error)
	Connect(ctx context.Context, c rpc.Connect) (rpc.ConnectResult, error)
}

func (s *server) pairingAddress(ctx context.Context, c *rpc.Call) (any, error) {
	pp, ok := s.p.(PeerPairing)
	if !ok {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.PairingAddressRequest
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.initialized(); err != nil {
		return nil, err
	}
	var out rpc.PairingAddress
	err := safely(func() (e error) { out.Address, e = pp.PairingAddress(ctx, in.Purpose); return })
	if err != nil {
		return nil, err
	}
	if out.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInternal)
	}
	return out, nil
}

func (s *server) connectPeer(ctx context.Context, c *rpc.Call) (any, error) {
	pp, ok := s.p.(PeerPairing)
	if !ok {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.Connect
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.initialized(); err != nil {
		return nil, err
	}
	var out rpc.ConnectResult
	err := safely(func() (e error) { out, e = pp.Connect(ctx, in); return })
	if err != nil {
		return nil, err
	}
	if out.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInternal)
	}
	return out, nil
}
