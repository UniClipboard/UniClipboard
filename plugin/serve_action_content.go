package plugin

import (
	"context"
	"io"
	"sync"
	"sync/atomic"
	"time"

	"github.com/uniclipboard/unic/plugin/rpc"
)

func (a *actionCtx) Parts() []ActionPart {
	out := make([]ActionPart, len(a.parts))
	for i, p := range a.parts {
		out[i] = ActionPart{MIME: p.MIME, File: p.File, Name: p.Name, Size: int64(p.Size)}
	}
	return out
}
func (a *actionCtx) OpenPart(ctx context.Context, index int) (io.ReadCloser, error) {
	if index < 0 || index >= len(a.parts) || a.ctx.Err() != nil {
		return nil, ErrInvalidArgument
	}
	merged, cancel := context.WithCancel(a.ctx)
	stop := context.AfterFunc(ctx, cancel)
	reader, err := a.call.Conn.CallStream(merged, rpc.CallOptions{Method: rpc.MethodOpenActionPart, Parent: a.call.ID, Timeout: 30 * time.Second, Params: rpc.OpenActionPart{Index: index}}, rpc.PurposeActionPart, uint64(a.parts[index].Size))
	if err != nil {
		stop()
		cancel()
		return nil, err
	}
	out := &actionSourceReader{ReadCloser: reader, done: func() { stop(); cancel() }}
	out.complete.Store(a.parts[index].Size == 0)
	out.release = func() error {
		if a.ctx.Err() != nil {
			return nil
		} // Invocation-wide revocation already owns cleanup.
		closeCtx, stop := context.WithTimeout(a.ctx, 5*time.Second)
		defer stop()
		_, err := a.call.Conn.Call(closeCtx, rpc.CallOptions{Method: rpc.MethodCloseActionPart, Parent: a.call.ID, Timeout: 5 * time.Second, Params: rpc.OpenActionPart{Index: index}}, nil)
		return err
	}
	return out, nil
}

type actionSourceReader struct {
	io.ReadCloser
	done     func()
	release  func() error
	complete atomic.Bool
	once     sync.Once
	err      error
}

func (r *actionSourceReader) Read(b []byte) (int, error) {
	n, err := r.ReadCloser.Read(b)
	if err == io.EOF {
		r.complete.Store(true)
	}
	return n, err
}
func (r *actionSourceReader) Close() error {
	r.once.Do(func() {
		r.err = r.ReadCloser.Close()
		if !r.complete.Load() {
			if err := r.release(); r.err == nil {
				r.err = err
			}
		}
		r.done()
	})
	return r.err
}
