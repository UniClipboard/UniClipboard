package plugin

import (
	"bytes"
	"context"
	"io"
	"sync"
	"time"

	"github.com/uniclipboard/unic/plugin/rpc"
)

// Serving a sync backend (external-channel profile). The plugin implements
// SyncBackend; ReceiverAware and ChannelLeaver are used when present.

// serveBackendInit gives a ReceiverAware backend its Receiver. Called from init.
func (s *server) wireBackend() {
	s.wirePeer()
	if ra, ok := s.p.(ReceiverAware); ok && s.spec.ExternalChannels {
		ra.SetReceiver(remoteReceiver{s})
	}
}

func (s *server) submit(ctx context.Context, c *rpc.Call) (any, error) {
	sb, ok := s.p.(SyncBackend)
	if !ok {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.Submit
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.started(); err != nil {
		return nil, err
	}
	view := SyncView{SourceApplication: in.SourceApplication.Clone(), ID: in.TransferID, TimestampMs: int64(in.TimestampMs), Targets: in.Targets}
	var total uint64
	for _, p := range in.Parts {
		if p.Stream.Purpose != rpc.PurposeSyncPart {
			return nil, rpc.NewError(rpc.CodeInvalidArgument)
		}
		rs, err := c.Conn.Expect(ctx, c.ID, p.Stream, rpc.MaxSyncBytes-total)
		if err != nil {
			return nil, err
		}
		var buf bytes.Buffer
		_, err = io.Copy(&buf, rs)
		_ = rs.Close()
		if err != nil {
			return nil, err
		}
		total += uint64(buf.Len())
		data := buf.Bytes()
		view.Parts = append(view.Parts, SyncPart{MIME: p.MIME, Size: int64(len(data)),
			Open: func(context.Context) (io.ReadCloser, error) { return io.NopCloser(bytes.NewReader(data)), nil }})
	}
	var queued bool
	// The backend keeps working after this call returns, so its host handle
	// must not depend on the call's context.
	host := remoteSyncHost{s: s, id: in.TransferID}
	err := safely(func() (e error) { queued, e = sb.Submit(ctx, view, host); return })
	if err != nil {
		return nil, err
	}
	return rpc.SubmitResult{Queued: queued}, nil
}

func (s *server) leaveChannel(ctx context.Context, c *rpc.Call) (any, error) {
	l, ok := s.p.(ChannelLeaver)
	if !ok || !s.spec.RemoteRevocation {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.LeaveChannel
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.started(); err != nil {
		return nil, err
	}
	return nil, safely(func() error { return l.LeaveChannel(ctx, in.ChannelID) })
}

type remoteSyncHost struct {
	s  *server
	id string
}

func (h remoteSyncHost) Register(targets []string) error {
	_, err := h.s.conn.Call(context.Background(), rpc.CallOptions{Method: rpc.MethodRegister, Timeout: 5 * time.Second,
		Params: rpc.Register{TransferID: h.id, Targets: targets}}, nil)
	return err
}

func (h remoteSyncHost) Report(target string, state SyncState, _ error) {
	_, _ = h.s.conn.Call(context.Background(), rpc.CallOptions{Method: rpc.MethodReport, Timeout: 5 * time.Second,
		Params: rpc.Report{TransferID: h.id, Target: target, State: string(state)}}, nil)
}

type remoteReceiver struct{ s *server }

var recvMu sync.Mutex // one inbound delivery at a time keeps stream use simple

func (r remoteReceiver) Receive(ctx context.Context, channelID string, rv Received) error {
	if len(rv.Parts) == 0 || len(rv.Parts) > rpc.MaxReceiveParts {
		return ErrInvalidArgument
	}
	recvMu.Lock()
	defer recvMu.Unlock()
	conn := r.s.conn
	id, err := conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodReceive, Timeout: 30 * time.Second,
		Build: func(id uint64) (any, error) {
			out := rpc.Receive{SourceApplication: rv.SourceApplication.Clone(), ChannelID: channelID, Transfer: rv.Transfer, View: rv.View}
			for _, p := range rv.Parts {
				ss, d, err := conn.NewSend(conn.Context(), id, rpc.PurposeReceivePart, uint64(len(p.Data)))
				if err != nil {
					return nil, err
				}
				data := p.Data
				go func() {
					if _, err := ss.Write(data); err == nil {
						_ = ss.Close()
					}
				}()
				out.Parts = append(out.Parts, rpc.ReceivePart{MIME: p.MIME, Stream: d})
			}
			return out, nil
		}}, nil)
	if err != nil {
		return err
	}
	conn.ReleaseRoot(id)
	return nil
}
