package pluginproc

import (
	"context"
	"crypto/subtle"
	"io"
	"sync"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Single-target send (sync-backend-contract §4). The backend may only
// authorize and open parts under a live backend.send of this proxy, using the
// identifiers that send carried; nothing it asserts is taken on trust.

var _ plugin.PeerDispatcher = peerM{}

const maxOpenParts = 4 // streams a send may hold open at once

func (m peerM) Send(ctx context.Context, s rpc.Send, src plugin.SendSource) error {
	return m.p.send(ctx, s, src)
}

type activeSend struct {
	mu     sync.Mutex
	in     rpc.Send
	src    plugin.SendSource
	grant  string
	opened map[int]bool
	live   int
}

func (p *Proxy) send(ctx context.Context, in rpc.Send, src plugin.SendSource) error {
	if err := in.Validate(); err != nil {
		return err
	}
	s := p.current()
	if s == nil {
		return ErrUnavailable
	}
	a := &activeSend{in: in, src: src, opened: map[int]bool{}}
	var res rpc.SendResult
	var root uint64
	id, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodSend, Timeout: p.callTimeout(ctx, p.opts.SendTimeout),
		Build: func(id uint64) (any, error) {
			root = id
			p.bmu.Lock()
			if p.sends == nil {
				p.sends = map[uint64]*activeSend{}
			}
			p.sends[id] = a
			p.bmu.Unlock()
			return in, nil
		}}, &res)
	if root != 0 {
		p.bmu.Lock()
		delete(p.sends, root)
		p.bmu.Unlock()
	}
	if err != nil {
		return err
	}
	s.conn.ReleaseRoot(id)
	return res.Validate()
}

func (p *Proxy) sendFor(c *rpc.Call) *activeSend {
	p.bmu.Lock()
	defer p.bmu.Unlock()
	return p.sends[c.Parent]
}

func eqSecret(a, b string) bool { return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1 }

func (p *Proxy) authorizeSend(ctx context.Context, c *rpc.Call) (any, error) {
	var in rpc.Authorize
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	a := p.sendFor(c)
	if a == nil {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	if in.DispatchID != a.in.DispatchID || in.ConnectionID != a.in.ConnectionID ||
		!eqSecret(in.ContentHandle, a.in.ContentHandle) || !eqSecret(in.SessionToken, a.in.SessionToken) {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	a.mu.Lock()
	if a.grant != "" {
		a.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeConflict) // one grant per send
	}
	a.grant = "pending"
	a.mu.Unlock()
	g, err := a.src.Authorize(ctx, in)
	if err == nil {
		err = g.Validate()
	}
	a.mu.Lock()
	if err != nil {
		a.grant = "" // a refused send may ask again only if the host says so
		a.mu.Unlock()
		return nil, err
	}
	a.grant = g.GrantID
	a.mu.Unlock()
	return g, nil
}

func (p *Proxy) openSendPart(ctx context.Context, c *rpc.Call) (any, error) {
	var in rpc.OpenSendPart
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	a := p.sendFor(c)
	if a == nil {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	a.mu.Lock()
	switch {
	case a.grant == "" || a.grant == "pending" || !eqSecret(a.grant, in.GrantID):
		a.mu.Unlock()
		return nil, rpc.NewError(rpc.CodePermission)
	case in.Index >= len(a.in.Clip.Parts):
		a.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	case a.opened[in.Index]:
		a.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeConflict)
	case a.live >= maxOpenParts:
		a.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeExhausted)
	}
	a.opened[in.Index] = true
	a.live++
	a.mu.Unlock()
	release := func() { a.mu.Lock(); a.live--; a.mu.Unlock() }

	size := uint64(a.in.Clip.Parts[in.Index].Size)
	rc, err := a.src.OpenPart(ctx, in.GrantID, in.Index)
	if err != nil {
		release()
		return nil, err
	}
	if size == 0 {
		// An empty part carries no frames: the credit that paces a stream never
		// comes for zero bytes, so its END could overtake the response that
		// announces it. The descriptor (stream_id 0) just says "empty".
		rc.Close()
		release()
		return rpc.StreamDesc{StreamID: 0, Size: 0, Purpose: rpc.PurposeSyncPart}, nil
	}
	ss, d, err := c.Conn.NewSend(c.Conn.Context(), c.Parent, rpc.PurposeSyncPart, size)
	if err != nil {
		rc.Close()
		release()
		return nil, err
	}
	go func() {
		defer rc.Close()
		defer release()
		if _, err := io.Copy(ss, rc); err == nil {
			_ = ss.Close()
		}
	}()
	return d, nil
}
