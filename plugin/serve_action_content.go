package plugin

import (
	"bytes"
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
	var desc rpc.StreamDesc
	_, err := a.call.Conn.Call(merged, rpc.CallOptions{Method: rpc.MethodOpenActionPart, Parent: a.call.ID, Timeout: 30 * time.Second, Params: rpc.OpenActionPart{Index: index}}, &desc)
	if err != nil {
		stop()
		cancel()
		return nil, err
	}
	if desc.Purpose != rpc.PurposeActionPart || uint64(desc.Size) != uint64(a.parts[index].Size) {
		stop()
		cancel()
		return nil, ErrInvalidArgument
	}
	var reader io.ReadCloser
	if desc.Size == 0 && desc.StreamID == 0 {
		reader = io.NopCloser(bytes.NewReader(nil))
	} else {
		reader, err = a.call.Conn.Expect(merged, a.call.ID, desc, uint64(a.parts[index].Size))
	}
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
		// The close response is a writer barrier. Keep receive credit valid until
		// all already-authorized DATA precedes that response on the wire.
		if !r.complete.Load() {
			r.err = r.release()
		}
		if err := r.ReadCloser.Close(); r.err == nil {
			r.err = err
		}
		r.done()
	})
	return r.err
}
