package pluginproc

import (
	"context"
	"github.com/uniclipboard/unic/plugin/rpc"
	"io"
)

func (p *Proxy) openActionPart(s *session, ctx context.Context, c *rpc.Call) (any, error) {
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
	col.readParts[in.Index] = true
	col.liveParts++
	col.mu.Unlock()
	release := func() { col.mu.Lock(); col.liveParts--; col.mu.Unlock() }
	rc, err := col.source.OpenPart(col.ctx, in.Index)
	if err != nil {
		release()
		return nil, rpc.NewError(rpc.CodeUnavailable)
	}
	size := parts[in.Index].Size
	if size == 0 {
		_ = rc.Close()
		release()
		return rpc.StreamDesc{Purpose: rpc.PurposeActionPart}, nil
	}
	ss, d, err := c.Conn.NewSend(col.ctx, c.Parent, rpc.PurposeActionPart, uint64(size))
	if err != nil {
		_ = rc.Close()
		release()
		return nil, err
	}
	go func() {
		defer rc.Close()
		defer release()
		stop := context.AfterFunc(col.ctx, func() { _ = rc.Close() })
		defer stop()
		if n, err := io.Copy(ss, rc); err == nil && n == size {
			_ = ss.Close()
		} else {
			_ = c.Conn.Close() // A failed source must not leave its peer waiting for stream credit.
		}
	}()
	return d, nil
}
