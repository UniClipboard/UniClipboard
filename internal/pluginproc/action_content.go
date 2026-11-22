package pluginproc

import (
	"context"
	"errors"
	"io"
	"sync"

	"github.com/uniclipboard/unic/plugin/rpc"
)

type actionPartTransfer struct {
	cancel context.CancelFunc
	done   chan struct{}
	once   sync.Once
}

func (p *Proxy) closeActionPart(s *session, ctx context.Context, c *rpc.Call) (any, error) {
	var in rpc.OpenActionPart
	if rpc.Decode(c.Params, &in) != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	value, ok := s.actions.Load(c.Parent)
	if !ok {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	col := value.(*collector)
	col.mu.Lock()
	part := col.partTransfers[in.Index]
	col.mu.Unlock()
	if part == nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	part.cancel()
	select {
	case <-part.done:
		return rpc.Empty{}, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func (p *Proxy) openActionPart(s *session, _ context.Context, c *rpc.Call) (any, error) {
	var in rpc.OpenActionPart
	if rpc.Decode(c.Params, &in) != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	value, ok := s.actions.Load(c.Parent)
	if !ok {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	col := value.(*collector)
	col.mu.Lock()
	if col.source == nil || col.ctx.Err() != nil {
		col.mu.Unlock()
		return nil, rpc.NewError(rpc.CodePermission)
	}
	parts := col.source.Parts()
	if in.Index < 0 || in.Index >= len(parts) || col.readParts[in.Index] {
		col.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if col.liveParts >= 4 {
		col.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeExhausted)
	}
	partCtx, cancel := context.WithCancel(col.ctx)
	part := &actionPartTransfer{cancel: cancel, done: make(chan struct{})}
	col.readParts[in.Index] = true
	col.partTransfers[in.Index] = part
	col.liveParts++
	col.mu.Unlock()
	release := func() {
		part.once.Do(func() { cancel(); col.mu.Lock(); col.liveParts--; col.mu.Unlock(); close(part.done) })
	}
	rc, err := col.source.OpenPart(partCtx, in.Index)
	if err != nil {
		release()
		return nil, rpc.NewError(rpc.CodeUnavailable)
	}
	stop := context.AfterFunc(partCtx, func() { _ = rc.Close() })
	finish := func() { stop(); _ = rc.Close(); release() }
	size := parts[in.Index].Size
	if size == 0 {
		finish()
		return rpc.StreamDesc{Purpose: rpc.PurposeActionPart}, nil
	}
	stream, desc, err := c.Conn.NewSend(partCtx, c.Parent, rpc.PurposeActionPart, uint64(size))
	if err != nil {
		finish()
		return nil, err
	}
	go func() {
		defer finish()
		n, err := io.Copy(stream, rc)
		if partCtx.Err() != nil || col.ctx.Err() != nil || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
			_ = stream.Close()
			return
		}
		if err == nil && n == size {
			_ = stream.Close()
			return
		}
		_ = c.Conn.Close() // Genuine incomplete source; cancellation takes the branch above.
	}()
	return desc, nil
}
