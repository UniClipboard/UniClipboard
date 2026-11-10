package tcnet

import (
	"context"
	"fmt"
	"net"
	"sync"
	"time"

	"github.com/tailscale/tailcat"
	"tailscale.com/types/key"
)

// An ephemeral endpoint has a random key and a random pre-shared key, persists
// nothing and is never the device's own address. It carries one temporary
// transfer (0022). The browser transfer page dials port 1, so a listener accepts
// every virtual port.

// EphemeralListener accepts tunnel connections on any port.
type EphemeralListener struct {
	srv  *tailcat.Server
	addr string

	conns chan net.Conn
	done  chan struct{}
	once  sync.Once
}

// ListenEphemeral starts a one-shot tailcat endpoint. Its address is a secret.
func ListenEphemeral(logf func(string, ...any)) (*EphemeralListener, error) {
	l := &EphemeralListener{conns: make(chan net.Conn), done: make(chan struct{})}
	l.srv = &tailcat.Server{Key: key.NewNode(), PresharedKey: tailcat.NewPresharedKey(), Logf: logf}
	l.srv.OnTCP = func(uint16) func(net.Conn) {
		return func(c net.Conn) {
			select {
			case l.conns <- c:
			case <-l.done:
				_ = c.Close()
			}
		}
	}
	if err := l.srv.Start(); err != nil {
		_ = l.srv.Close()
		return nil, fmt.Errorf("start tailcat: %w", err)
	}
	l.addr = string(l.srv.TailcatAddr())
	return l, nil
}

// TailcatAddr is the address a peer dials.
func (l *EphemeralListener) TailcatAddr() string { return l.addr }

// Accept implements net.Listener.
func (l *EphemeralListener) Accept() (net.Conn, error) {
	select {
	case c := <-l.conns:
		return c, nil
	case <-l.done:
		return nil, net.ErrClosed
	}
}

// Close implements net.Listener.
func (l *EphemeralListener) Close() error {
	var err error
	l.once.Do(func() {
		close(l.done)
		err = l.srv.Close()
	})
	return err
}

// Addr implements net.Listener. The tunnel address is not an IP address.
func (l *EphemeralListener) Addr() net.Addr { return ephemeralAddr(l.addr) }

type ephemeralAddr string

func (ephemeralAddr) Network() string  { return "tailcat" }
func (a ephemeralAddr) String() string { return "tailcat" } // never print the secret address

// DialEphemeral connects to addr with a fresh client key. It retries until ctx
// ends, because one attempt gives up after the client's own connect timeout.
// Closing the returned connection also closes the client.
func DialEphemeral(ctx context.Context, addr string, port uint16, logf func(string, ...any)) (net.Conn, error) {
	if _, err := tailcat.ParseAddr(tailcat.Addr(addr)); err != nil {
		return nil, ErrBadAddress
	}
	cl := &tailcat.Client{Server: tailcat.Addr(addr), Key: key.NewNode(), Logf: logf}
	for {
		c, err := cl.DialTCPPort(ctx, port)
		if err == nil {
			return &clientConn{Conn: c, cl: cl}, nil
		}
		if ctx.Err() != nil {
			_ = cl.Close()
			return nil, ctx.Err()
		}
		select {
		case <-ctx.Done():
			_ = cl.Close()
			return nil, ctx.Err()
		case <-time.After(500 * time.Millisecond):
		}
	}
}

type clientConn struct {
	net.Conn
	cl *tailcat.Client
}

func (c *clientConn) CloseWrite() error {
	if cw, ok := c.Conn.(interface{ CloseWrite() error }); ok {
		return cw.CloseWrite()
	}
	return nil
}

func (c *clientConn) Close() error {
	err := c.Conn.Close()
	_ = c.cl.Close()
	return err
}
