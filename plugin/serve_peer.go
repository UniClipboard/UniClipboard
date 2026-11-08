package plugin

import (
	"bytes"
	"context"
	"io"

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
