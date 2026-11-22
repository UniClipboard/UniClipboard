package plugin

import (
	"bytes"
	"context"
	"github.com/uniclipboard/unic/plugin/rpc"
	"io"
	"time"
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
	var d rpc.StreamDesc
	_, err := a.call.Conn.Call(merged, rpc.CallOptions{Method: rpc.MethodOpenActionPart, Parent: a.call.ID, Timeout: 30 * time.Second, Params: rpc.OpenActionPart{Index: index}}, &d)
	if err != nil {
		stop()
		cancel()
		return nil, err
	}
	if d.Purpose != rpc.PurposeActionPart || uint64(d.Size) != uint64(a.parts[index].Size) {
		stop()
		cancel()
		return nil, ErrInvalidArgument
	}
	var reader io.ReadCloser
	if d.Size == 0 && d.StreamID == 0 {
		reader = io.NopCloser(bytes.NewReader(nil))
	} else {
		reader, err = a.call.Conn.Expect(merged, a.call.ID, d, uint64(a.parts[index].Size))
	}
	if err != nil {
		stop()
		cancel()
		return nil, err
	}
	return &actionSourceReader{ReadCloser: reader, done: func() { stop(); cancel() }}, nil
}

type actionSourceReader struct {
	io.ReadCloser
	done func()
}

func (r *actionSourceReader) Close() error { err := r.ReadCloser.Close(); r.done(); return err }
