//go:build unic_e2e

package main

import (
	"context"
	"net"
	"sync"
)

// e2eTransport reuses the host's existing transport seam. The real session,
// handshake, wire, Engine, and clipboard pipeline run over a local TCP socket;
// tests do not require a public DERP service to exercise diagnostics.
type e2eTransport struct {
	mu       sync.Mutex
	address  string
	listener net.Listener
}

func newE2ETransport() (*e2eTransport, error) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return nil, err
	}
	return &e2eTransport{address: ln.Addr().String(), listener: ln}, nil
}
func (t *e2eTransport) Start() error                     { return nil }
func (t *e2eTransport) Addr() string                     { return t.address }
func (t *e2eTransport) Binding(a string) ([]byte, error) { return []byte("loop\x00" + a), nil }
func (t *e2eTransport) Listen(context.Context) (net.Listener, error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.listener != nil {
		ln := t.listener
		t.listener = nil
		return ln, nil
	}
	return net.Listen("tcp", t.address)
}
func (t *e2eTransport) Dial(ctx context.Context, a string) (net.Conn, error) {
	return (&net.Dialer{}).DialContext(ctx, "tcp", a)
}
func (t *e2eTransport) Close() error {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.listener != nil {
		return t.listener.Close()
	}
	return nil
}
