package rpcstreams

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugin/rpc"
)

// Independent root parameters and source responses share one replay ledger.
func TestSourceDescriptorRegistrationE2E(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	a, b := net.Pipe()
	allocated := make(chan struct{})
	host := rpc.NewConn(a, rpc.Options{Role: rpc.RoleEngine, Limits: rpc.DefaultLimits, Handler: func(ctx context.Context, c *rpc.Call) (any, error) {
		var in rpc.OpenActionPart
		if err := json.Unmarshal(c.Params, &in); err != nil {
			return nil, err
		}
		stream, desc, err := c.Conn.NewSend(c.Conn.Context(), c.Parent, rpc.PurposeActionPart, 8)
		if err != nil {
			return nil, err
		}
		if in.Index == 0 {
			close(allocated)
			// A separate root carries a descriptor in its parameters while
			// the source descriptor response is still pending.
			_, err := c.Conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodApplyState, Timeout: time.Second, Build: func(id uint64) (any, error) {
				stream, desc, err := c.Conn.NewSend(c.Conn.Context(), id, rpc.PurposeSyncState, 8)
				if err != nil {
					return nil, err
				}
				go func() { _, _ = io.WriteString(stream, "12345678"); _ = stream.Close() }()
				return desc, nil
			}}, nil)
			if err != nil {
				return nil, err
			}
		}
		go func() { _, _ = io.WriteString(stream, "12345678"); _ = stream.Close() }()
		return desc, nil
	}})
	defer func() { _ = host.Close() }()
	peer := rpc.NewConn(b, rpc.Options{Role: rpc.RolePlugin, Limits: rpc.DefaultLimits, Handler: func(ctx context.Context, c *rpc.Call) (any, error) {
		if c.Method == rpc.MethodApplyState {
			var desc rpc.StreamDesc
			if err := json.Unmarshal(c.Params, &desc); err != nil {
				return nil, err
			}
			r, err := c.Conn.Expect(ctx, c.ID, desc, 8)
			if err != nil {
				return nil, err
			}
			_, err = io.Copy(io.Discard, r)
			if closeErr := r.Close(); err == nil {
				err = closeErr
			}
			if err != nil {
				return nil, err
			}
			var replay *rpc.Error
			if _, err = c.Conn.Expect(ctx, c.ID, desc, 8); !errors.As(err, &replay) || replay.Code != rpc.CodeInvalidState {
				return nil, errors.New("retired stream ID replay accepted")
			}
			return rpc.Empty{}, nil
		}

		done := make(chan error, 2)
		open := func(method string, index int) {
			var desc rpc.StreamDesc
			_, err := c.Conn.Call(ctx, rpc.CallOptions{Method: method, Parent: c.ID, Params: rpc.OpenActionPart{Index: index}, Timeout: time.Second}, &desc)
			if err != nil {
				done <- err
				return
			}
			r, err := c.Conn.Expect(ctx, c.ID, desc, 8)
			if err != nil {
				done <- err
				return
			}
			data, err := io.ReadAll(r)
			if closeErr := r.Close(); err == nil {
				err = closeErr
			}
			if err == nil && string(data) != "12345678" {
				err = io.ErrUnexpectedEOF
			}
			done <- err
		}
		go open(rpc.MethodOpenActionPart, 0)
		<-allocated
		go open(rpc.MethodOpenSendPart, 1)
		for i := 0; i < 2; i++ {
			if err := <-done; err != nil {
				return nil, err
			}
		}
		return rpc.Empty{}, nil
	}})
	defer func() { _ = peer.Close() }()
	_, err := host.Call(ctx, rpc.CallOptions{Method: rpc.MethodRunAction, Params: rpc.Empty{}, Timeout: 2 * time.Second}, nil)
	if err != nil {
		t.Fatal("independent descriptor callers raced registration:", err)
	}
}
