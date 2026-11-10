//go:build unic_e2e

package main

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"io"
	"net"
	"sync"
)

// e2eTransport reuses the host's existing transport seam. The real session,
// handshake, wire, Engine, and clipboard pipeline run over a local TCP socket;
// tests do not require a public DERP service to exercise diagnostics.
type e2eTransport struct {
	mu          sync.Mutex
	address     string
	listener    net.Listener
	path        string
	conns       []net.Conn
	dialTimeout bool
	partial     int64
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
		return &trackedListener{Listener: ln, t: t}, nil
	}
	ln, err := net.Listen("tcp", t.address)
	if err != nil {
		return nil, err
	}
	return &trackedListener{Listener: ln, t: t}, nil
}
func (t *e2eTransport) Dial(ctx context.Context, a string) (net.Conn, error) {
	t.mu.Lock()
	fail := t.dialTimeout
	t.mu.Unlock()
	if fail {
		return nil, context.DeadlineExceeded
	}
	c, err := (&net.Dialer{}).DialContext(ctx, "tcp", a)
	if err == nil {
		c = &faultConn{Conn: c, t: t}
		t.mu.Lock()
		t.conns = append(t.conns, c)
		t.mu.Unlock()
	}
	return c, err
}
func (t *e2eTransport) Close() error {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.listener != nil {
		return t.listener.Close()
	}
	return nil
}

func (t *e2eTransport) setPath(path string) { t.mu.Lock(); t.path = path; t.mu.Unlock() }
func (t *e2eTransport) Path(net.Conn, bool) string {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.path == "" {
		return "unknown"
	}
	return t.path
}
func (t *e2eTransport) disconnect() {
	t.mu.Lock()
	cs := t.conns
	t.conns = nil
	t.mu.Unlock()
	for _, c := range cs {
		_ = c.Close()
	}
}

type trackedListener struct {
	net.Listener
	t *e2eTransport
}

func (l *trackedListener) Accept() (net.Conn, error) {
	c, err := l.Listener.Accept()
	if err == nil {
		c = &faultConn{Conn: c, t: l.t}
		l.t.mu.Lock()
		l.t.conns = append(l.t.conns, c)
		l.t.mu.Unlock()
	}
	return c, err
}

type faultConn struct {
	net.Conn
	t         *e2eTransport
	failAfter int64
}

func (c *faultConn) Write(p []byte) (int, error) {
	if c.failAfter > 0 {
		n, err := c.Conn.Write(p[:min(int64(len(p)), c.failAfter)])
		c.failAfter = 0
		_ = c.Conn.Close()
		if err == nil {
			err = io.ErrUnexpectedEOF
		}
		return n, err
	}
	if len(p) > 4 && int(binary.BigEndian.Uint32(p[:4])) == len(p)-4 {
		var header struct {
			T string `json:"t"`
		}
		if json.Unmarshal(p[4:], &header) == nil && header.T == "clip" {
			c.t.mu.Lock()
			c.failAfter = c.t.partial
			c.t.partial = 0
			c.t.mu.Unlock()
		}
	}
	return c.Conn.Write(p)
}
