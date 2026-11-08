package pluginproc

import (
	"bytes"
	"context"
	"errors"
	"io"
	"sync"
	"time"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Inbound clips (sync-backend-contract §5). A receive_id is single-use,
// short-lived and bounded in number; parts must arrive in order with the
// sizes the admitted header declared.

var _ plugin.PeerReceiverAware = peerM{}

func (m peerM) SetPeerReceiver(r plugin.PeerReceiver) {
	m.p.bmu.Lock()
	m.p.precv = r
	m.p.bmu.Unlock()
}

func (m peerM) SetPeerAuthority(a plugin.PeerAuthority) {
	m.p.bmu.Lock()
	m.p.pauth = a
	m.p.bmu.Unlock()
}

// identity serves the three handshake methods through the Engine's authority.
func (p *Proxy) identity(ctx context.Context, c *rpc.Call) (any, error) {
	p.bmu.Lock()
	a := p.pauth
	p.bmu.Unlock()
	if a == nil {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	switch c.Method {
	case rpc.MethodIdentityHello:
		var in rpc.IdentityHello
		if rpc.Decode(c.Params, &in) != nil || in.Validate() != nil {
			return nil, rpc.NewError(rpc.CodeInvalidArgument)
		}
		return nilOnErr(a.IdentityHello(ctx, in))
	case rpc.MethodIdentityProve:
		var in rpc.IdentityProve
		if rpc.Decode(c.Params, &in) != nil || in.Validate() != nil {
			return nil, rpc.NewError(rpc.CodeInvalidArgument)
		}
		return nilOnErr(a.IdentityProve(ctx, in))
	default:
		var in rpc.CheckPeer
		if rpc.Decode(c.Params, &in) != nil || in.Validate() != nil {
			return nil, rpc.NewError(rpc.CodeInvalidArgument)
		}
		return nilOnErr(a.CheckPeer(ctx, in))
	}
}

func (p *Proxy) peerEvent(ctx context.Context, c *rpc.Call) (any, error) {
	p.bmu.Lock()
	a := p.pauth
	p.bmu.Unlock()
	var in rpc.PeerEvent
	if a == nil || rpc.Decode(c.Params, &in) != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	return nilOnErr(rpc.Empty{}, a.PeerEvent(ctx, in))
}

func (p *Proxy) introduce(ctx context.Context, c *rpc.Call) (any, error) {
	p.bmu.Lock()
	a := p.pauth
	p.bmu.Unlock()
	var in rpc.Introduce
	if a == nil || rpc.Decode(c.Params, &in) != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	return nilOnErr(rpc.Empty{}, a.Introduce(ctx, in))
}

// nilOnErr maps any error to a stable wire code; free text never crosses.
func nilOnErr[T any](v T, err error) (any, error) {
	if err != nil {
		var re *rpc.Error
		if errors.As(err, &re) {
			return nil, re
		}
		return nil, rpc.NewError(rpc.CodeInternal)
	}
	return v, nil
}

type recvGrant struct {
	sizes    []uint64
	deadline time.Time
}

type recvPart struct {
	body io.ReadCloser
}

type activeRecv struct {
	mu    sync.Mutex
	id    string
	sizes []uint64
	next  int // parts declared by the backend
	taken int // parts handed to the Engine
	ch    chan recvPart
}

type recvParts struct{ a *activeRecv }

func (r recvParts) Next(ctx context.Context) (int, io.ReadCloser, error) {
	a := r.a
	a.mu.Lock()
	i := a.taken
	a.mu.Unlock()
	if i >= len(a.sizes) {
		return 0, nil, io.EOF
	}
	select {
	case p := <-a.ch:
		a.mu.Lock()
		a.taken++
		a.mu.Unlock()
		return i, p.body, nil
	case <-ctx.Done():
		return 0, nil, ctx.Err()
	}
}

func (p *Proxy) purgeGrantsLocked(now time.Time) {
	for id, g := range p.rgrants {
		if now.After(g.deadline) {
			delete(p.rgrants, id)
		}
	}
}

func (p *Proxy) receiveOpen(ctx context.Context, c *rpc.Call) (any, error) {
	p.bmu.Lock()
	r := p.precv
	p.bmu.Unlock()
	var in rpc.ReceiveOpen
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil || r == nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	p.bmu.Lock()
	p.purgeGrantsLocked(time.Now())
	full := len(p.rgrants) >= rpc.MaxOpenReceives
	p.bmu.Unlock()
	if full {
		return rpc.ReceiveGrant{Decision: rpc.DecisionDiscard}, nil // the backend drains the declared length
	}
	g, err := r.OpenReceive(ctx, in)
	if err != nil {
		return nil, err
	}
	if err := g.Validate(); err != nil {
		return nil, err
	}
	if g.ReceiveID != nil {
		rg := &recvGrant{deadline: time.Now().Add(rpc.ReceiveOpenTTLMs * time.Millisecond)}
		for _, part := range in.Clip.Parts {
			rg.sizes = append(rg.sizes, uint64(part.Size))
		}
		p.bmu.Lock()
		if p.rgrants == nil {
			p.rgrants = map[string]*recvGrant{}
		}
		p.rgrants[*g.ReceiveID] = rg
		p.bmu.Unlock()
	}
	return g, nil
}

func (p *Proxy) receiveRoot(ctx context.Context, c *rpc.Call) (any, error) {
	p.bmu.Lock()
	r := p.precv
	p.bmu.Unlock()
	var in rpc.PeerReceive
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil || r == nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	p.bmu.Lock()
	g := p.rgrants[in.ReceiveID]
	delete(p.rgrants, in.ReceiveID) // single use, whatever happens next
	p.bmu.Unlock()
	if g == nil {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	if time.Now().After(g.deadline) {
		return nil, rpc.NewError("resource_expired")
	}
	a := &activeRecv{id: in.ReceiveID, sizes: g.sizes, ch: make(chan recvPart, 1)}
	p.bmu.Lock()
	if p.recvs == nil {
		p.recvs = map[uint64]*activeRecv{}
	}
	p.recvs[c.ID] = a
	p.bmu.Unlock()
	defer func() {
		p.bmu.Lock()
		delete(p.recvs, c.ID)
		p.bmu.Unlock()
		for { // bodies declared but never taken
			select {
			case x := <-a.ch:
				x.body.Close()
			default:
				return
			}
		}
	}()
	outcome, err := r.Receive(ctx, in.ReceiveID, recvParts{a})
	if err != nil {
		return nil, err
	}
	res := rpc.PeerReceiveResult{Outcome: outcome}
	if err := res.Validate(); err != nil {
		return nil, err
	}
	return res, nil
}

func (p *Proxy) receivePart(ctx context.Context, c *rpc.Call) (any, error) {
	var in rpc.PeerReceivePart
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	p.bmu.Lock()
	a := p.recvs[c.Parent]
	p.bmu.Unlock()
	if a == nil || !eqSecret(a.id, in.ReceiveID) {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	a.mu.Lock()
	if in.Index != a.next || in.Index >= len(a.sizes) {
		a.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeConflict) // out of order, repeated or beyond the header
	}
	a.next++
	a.mu.Unlock()
	want := a.sizes[in.Index]
	if uint64(in.Stream.Size) != want {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	var body io.ReadCloser
	if want == 0 {
		if in.Stream.StreamID != 0 {
			return nil, rpc.NewError(rpc.CodeInvalidArgument)
		}
		body = io.NopCloser(bytes.NewReader(nil))
	} else {
		rs, err := c.Conn.Expect(c.Conn.Context(), c.Parent, in.Stream, want)
		if err != nil {
			return nil, err
		}
		body = rs
	}
	select {
	case a.ch <- recvPart{body}:
		return nil, nil
	case <-ctx.Done():
		body.Close()
		return nil, ctx.Err()
	}
}
