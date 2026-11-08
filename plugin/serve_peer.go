package plugin

import (
	"bytes"
	"context"
	"io"
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
	rs.Close()
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
// first wire byte, then pull each part with h.OpenPart; returning nil means
// the whole clip was written locally, not that the peer applied it.
type PeerSender interface {
	Send(ctx context.Context, s rpc.Send, h SendHost) error
}

// SendHost is the backend's view of one backend.send call. It dies with it.
type SendHost interface {
	Authorize(ctx context.Context) (rpc.SendGrant, error)
	// OpenPart returns the bytes of part index under the grant; the caller
	// closes it. Each part opens once.
	OpenPart(ctx context.Context, grantID string, index int) (io.ReadCloser, error)
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
	if err := safely(func() error { return ps.Send(ctx, in, remoteSendHost{conn: c.Conn, root: c.ID, in: in}) }); err != nil {
		return nil, err
	}
	return rpc.SendResult{State: rpc.SendSent}, nil
}

// SendSource is the Engine side of one send: it authorizes the transfer and
// supplies part bytes. The host Send validates every request against the
// backend.send it belongs to before calling it.
type SendSource interface {
	Authorize(ctx context.Context, a rpc.Authorize) (rpc.SendGrant, error)
	OpenPart(ctx context.Context, grantID string, index int) (io.ReadCloser, error)
}

// PeerDispatcher is the Engine-facing form of PeerSender: it starts one
// backend.send and serves the backend's authorize/open requests from src.
type PeerDispatcher interface {
	Send(ctx context.Context, s rpc.Send, src SendSource) error
}
