package session

import (
	"context"
	"errors"
	"net"
	"testing"
	"time"
)

// blockedNet is a Transport whose Start waits until Close.
type blockedNet struct{ stop chan struct{} }

func (n *blockedNet) Start() error                                   { <-n.stop; return errors.New("closed") }
func (n *blockedNet) Close() error                                   { close(n.stop); return nil }
func (n *blockedNet) Listen(context.Context) (net.Listener, error)   { return nil, errors.New("down") }
func (n *blockedNet) Dial(context.Context, string) (net.Conn, error) { return nil, errors.New("down") }
func (n *blockedNet) Addr() string                                   { return "" }
func (n *blockedNet) Binding(string) ([]byte, error)                 { return nil, errors.New("down") }

// Close must cut a Start that is still waiting for the network short instead
// of waiting for it.
func TestCloseDoesNotWaitForStart(t *testing.T) {
	tr := &blockedNet{stop: make(chan struct{})}
	b := New(Config{Transport: tr})
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	_ = b.up(ctx) // Start is still blocked when the context ends
	done := make(chan error, 1)
	go func() { done <- b.Close(context.Background()) }()
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Close waited for a blocked Start")
	}
}
