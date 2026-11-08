package pluginproc

import (
	"bytes"
	"context"
	"io"
	"slices"
	"sync"
	"unicode/utf8"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// External-channel sync backend over RPC (sync-backend-contract §6a). Content
// moves in streams, never in control JSON. Only text/image representations
// travel; file parts are not part of this profile and are skipped.

type backM struct{ p *Proxy }
type leaveM struct{ p *Proxy }

func (m backM) Submit(ctx context.Context, v plugin.SyncView, h plugin.SyncHost) (bool, error) {
	return m.p.submit(ctx, v, h)
}
func (m backM) SetReceiver(r plugin.Receiver) { m.p.setReceiver(r) }
func (m leaveM) LeaveChannel(ctx context.Context, id string) error {
	return m.p.leaveChannel(ctx, id)
}

const maxTransfers = 256

// transfer is one Submit whose host stays reachable until every registered
// target is terminal.
type transfer struct {
	mu       sync.Mutex
	h        plugin.SyncHost
	allowed  map[string]bool
	pending  map[string]bool
	returned bool
}

func (t *transfer) finishedLocked() bool { return t.returned && len(t.pending) == 0 }

func (p *Proxy) setReceiver(r plugin.Receiver) {
	p.bmu.Lock()
	p.receiver = r
	p.bmu.Unlock()
}

func (p *Proxy) dropTransfer(id string, t *transfer) {
	p.bmu.Lock()
	if p.transfers[id] == t {
		delete(p.transfers, id)
	}
	p.bmu.Unlock()
}

func (p *Proxy) submit(ctx context.Context, v plugin.SyncView, h plugin.SyncHost) (bool, error) {
	s := p.current()
	if s == nil {
		return false, ErrUnavailable
	}
	var parts []plugin.SyncPart
	var total int64
	for _, sp := range v.Parts {
		if sp.File {
			continue
		}
		total += sp.Size
		parts = append(parts, sp)
	}
	if len(parts) == 0 || len(parts) > rpc.MaxSyncParts || total > rpc.MaxSyncBytes || len(v.Targets) == 0 || len(v.Targets) > rpc.MaxSyncTargets {
		return false, rpc.NewError(rpc.CodeLimitExceeded)
	}
	t := &transfer{h: h, allowed: map[string]bool{}, pending: map[string]bool{}}
	for _, x := range v.Targets {
		t.allowed[x] = true
	}
	p.bmu.Lock()
	if p.transfers == nil {
		p.transfers = map[string]*transfer{}
	}
	if len(p.transfers) >= maxTransfers || p.transfers[v.ID] != nil {
		p.bmu.Unlock()
		return false, rpc.NewError(rpc.CodeExhausted)
	}
	p.transfers[v.ID] = t
	p.bmu.Unlock()

	var res rpc.SubmitResult
	id, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodSubmit, Timeout: p.callTimeout(ctx, p.opts.ProcessTimeout),
		Build: func(id uint64) (any, error) {
			out := rpc.Submit{TransferID: v.ID, TimestampMs: rpc.U64(v.TimestampMs), Targets: slices.Clone(v.Targets)}
			for _, sp := range parts {
				rc, err := sp.Open(ctx)
				if err != nil {
					return nil, err
				}
				ss, d, err := s.conn.NewSend(s.conn.Context(), id, rpc.PurposeSyncPart, uint64(sp.Size))
				if err != nil {
					rc.Close()
					return nil, err
				}
				go func() {
					defer rc.Close()
					if _, err := io.Copy(ss, rc); err == nil {
						_ = ss.Close()
					}
				}()
				out.Parts = append(out.Parts, rpc.SubmitPart{MIME: sp.MIME, Size: rpc.U64(sp.Size), Stream: d})
			}
			return out, nil
		}}, &res)
	if err != nil {
		p.dropTransfer(v.ID, t)
		return false, err
	}
	s.conn.ReleaseRoot(id)
	t.mu.Lock()
	t.returned = true
	done := t.finishedLocked() || !res.Queued
	t.mu.Unlock()
	if done {
		p.dropTransfer(v.ID, t)
	}
	return res.Queued, nil
}

func (p *Proxy) leaveChannel(ctx context.Context, channelID string) error {
	s := p.current()
	if s == nil {
		return ErrUnavailable
	}
	_, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodLeaveChannel, Timeout: p.callTimeout(ctx, p.opts.ProcessTimeout),
		Params: rpc.LeaveChannel{ChannelID: channelID}}, nil)
	return err
}

func (p *Proxy) transferFor(id string) *transfer {
	p.bmu.Lock()
	defer p.bmu.Unlock()
	return p.transfers[id]
}

func validID(s string) bool { return s != "" && len(s) <= rpc.MaxIDBytes && utf8.ValidString(s) }

// handleBackend serves plugin→Engine backend calls. ok=false: not a backend method.
func (p *Proxy) handleBackend(ctx context.Context, c *rpc.Call) (res any, err error, ok bool) {
	switch c.Method {
	case rpc.MethodRegister:
		var in rpc.Register
		t := p.decodeTransfer(c, &in, in.TransferID)
		if t == nil {
			return nil, rpc.NewError(rpc.CodeInvalidArgument), true
		}
		return nil, p.register(t, in), true
	case rpc.MethodReport:
		return nil, p.report(c), true
	case rpc.MethodReceive:
		return nil, p.receive(ctx, c), true
	}
	return nil, nil, false
}

func (p *Proxy) decodeTransfer(c *rpc.Call, in *rpc.Register, _ string) *transfer {
	if err := rpc.Decode(c.Params, in); err != nil {
		return nil
	}
	return p.transferFor(in.TransferID)
}

func (p *Proxy) register(t *transfer, in rpc.Register) error {
	if err := in.Validate(); err != nil {
		return err
	}
	t.mu.Lock()
	for _, x := range in.Targets {
		if !validID(x) || !t.allowed[x] {
			t.mu.Unlock()
			return rpc.NewError(rpc.CodePermission)
		}
	}
	t.mu.Unlock()
	if err := t.h.Register(in.Targets); err != nil {
		return rpc.NewError(rpc.CodeInvalidArgument)
	}
	t.mu.Lock()
	for _, x := range in.Targets {
		t.pending[x] = true
	}
	t.mu.Unlock()
	return nil
}

var syncStates = map[string]plugin.SyncState{
	"queued": plugin.SyncQueued, "sending": plugin.SyncSending, "sent": plugin.SyncSent,
	"superseded": plugin.SyncSuperseded, "failed": plugin.SyncFailed,
}

func (p *Proxy) report(c *rpc.Call) error {
	var in rpc.Report
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return rpc.NewError(rpc.CodeInvalidArgument)
	}
	st, ok := syncStates[in.State]
	t := p.transferFor(in.TransferID)
	if !ok || t == nil {
		return rpc.NewError(rpc.CodeInvalidArgument)
	}
	t.mu.Lock()
	if !t.pending[in.Target] {
		t.mu.Unlock()
		return rpc.NewError(rpc.CodePermission) // never registered
	}
	terminal := st == plugin.SyncSent || st == plugin.SyncSuperseded || st == plugin.SyncFailed
	if terminal {
		delete(t.pending, in.Target)
	}
	done := t.finishedLocked()
	t.mu.Unlock()
	t.h.Report(in.Target, st, nil)
	if done {
		p.dropTransfer(in.TransferID, t)
	}
	return nil
}

func (p *Proxy) receive(ctx context.Context, c *rpc.Call) error {
	p.bmu.Lock()
	r := p.receiver
	p.bmu.Unlock()
	var in rpc.Receive
	if r == nil {
		return rpc.NewError(rpc.CodeUnsupported)
	}
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return rpc.NewError(rpc.CodeInvalidArgument)
	}
	rv := plugin.Received{Transfer: in.Transfer, View: in.View}
	var total int64
	for _, part := range in.Parts {
		rs, err := c.Conn.Expect(ctx, c.ID, part.Stream, uint64(rpc.MaxSyncBytes-total))
		if err != nil {
			return err
		}
		var buf bytes.Buffer
		_, err = io.Copy(&buf, rs)
		rs.Close()
		if err != nil {
			return err
		}
		total += int64(buf.Len())
		rv.Parts = append(rv.Parts, plugin.ReceivedPart{MIME: part.MIME, Data: buf.Bytes()})
	}
	if err := r.Receive(ctx, in.ChannelID, rv); err != nil {
		return rpc.NewError(rpc.CodeInvalidState) // declined: the backend drops it
	}
	return nil
}
