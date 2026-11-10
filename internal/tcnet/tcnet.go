// Package tcnet implements engine.Transport on top of tailcat: WireGuard
// tunnels between two devices, direct when NAT traversal succeeds and relayed
// through DERP otherwise, with no account and no control server.
//
// A device's tailcat address is the whole of its credentials: it embeds the
// WireGuard pre-shared key, so anyone who has it can connect. Treat it like a
// password.
package tcnet

import (
	"context"
	"fmt"
	"net"
	"sync"

	"github.com/tailscale/tailcat"
	"tailscale.com/types/key"

	"github.com/uniclipboard/unic/internal/session"
)

// ErrBadAddress means the text is not a tailcat address.
var ErrBadAddress = session.ErrBadAddress

// Port is the virtual TCP port the sync protocol listens on inside the tunnel.
const Port = 7777

type Transport struct {
	statusMu     sync.Mutex
	statusReady  bool
	statusClosed bool
	dir          string
	logf         func(string, ...any)

	srv       *tailcat.Server
	clientKey key.NodePrivate

	mu      sync.Mutex
	clients map[string]*tailcat.Client
	addr    string
	creds   credentials
}

// New prepares the transport with credentials kept in dir, the backend's own
// private instance directory. The three keys are made together and published
// in one atomic write, so a failure leaves either all of them or none. A
// credentials file that exists but cannot be trusted is an error: new keys are
// never generated in its place, since that would silently change the address
// every paired device has stored.
func New(dir string, logf func(string, ...any)) (*Transport, error) {
	t := &Transport{dir: dir, logf: logf, clients: map[string]*tailcat.Client{}}
	c, err := loadCredentials(dir)
	if err != nil {
		return nil, err
	}
	t.creds = c
	if t.clientKey, err = parseNodeKey(c.ClientKey); err != nil {
		return nil, ErrBadCredentials
	}
	srvKey, err := parseNodeKey(c.ServerKey)
	if err != nil {
		return nil, ErrBadCredentials
	}
	var psk tailcat.PresharedKey
	if psk.UnmarshalText([]byte(c.PSK)) != nil {
		return nil, ErrBadCredentials
	}
	t.srv = &tailcat.Server{Key: srvKey, PresharedKey: psk, Logf: logf}

	// Pin the DERP region the address was first issued with. Without this a
	// laptop that wakes up on another network would pick another nearest
	// relay and silently change the address its peers have stored.
	if c.Addr != "" {
		if ci, err := tailcat.ParseAddr(tailcat.Addr(c.Addr)); err == nil && len(ci.Region) == 1 {
			t.srv.Region = ci.Region[0]
		}
	}
	return t, nil
}

// Start brings the tunnel endpoint up and records the address peers use.
func (t *Transport) Start() error {
	if err := t.srv.Start(); err != nil {
		return fmt.Errorf("start tailcat: %w", err)
	}
	t.statusMu.Lock()
	t.statusReady = !t.statusClosed
	t.statusMu.Unlock()
	addr := string(t.srv.TailcatAddr())
	t.mu.Lock()
	t.addr = addr
	t.mu.Unlock()
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.creds.Addr == addr {
		return nil
	}
	c := t.creds
	c.Addr = addr
	if err := saveCredentials(t.dir, c); err != nil {
		return err
	}
	t.creds = c
	return nil
}

func (t *Transport) Close() error {
	t.statusMu.Lock()
	defer t.statusMu.Unlock()
	t.statusClosed = true
	return t.srv.Close()
}

func (t *Transport) Listen(ctx context.Context) (net.Listener, error) {
	return t.srv.Listen(ctx, "tcp", fmt.Sprintf(":%d", Port))
}

// Addr is this device's address; empty before Start.
func (t *Transport) Addr() string {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.addr
}

func (t *Transport) Dial(ctx context.Context, addr string) (net.Conn, error) {
	if _, err := tailcat.ParseAddr(tailcat.Addr(addr)); err != nil {
		return nil, ErrBadAddress
	}
	t.mu.Lock()
	c := t.clients[addr]
	if c == nil {
		c = &tailcat.Client{Server: tailcat.Addr(addr), Key: t.clientKey, Logf: t.logf}
		t.clients[addr] = c
	}
	t.mu.Unlock()
	return c.DialTCPPort(ctx, Port)
}

// Binding is the identity protocol's context_binding for connections to the
// server at addr: a domain label and that server's node public key, which
// the dialer reads from the address and the server knows as its own. The
// address's pre-shared key is not part of it. Freshness per connection comes
// from the two nonces of the handshake, not from this value.
func (t *Transport) Binding(addr string) ([]byte, error) {
	ci, err := tailcat.ParseAddr(tailcat.Addr(addr))
	if err != nil {
		return nil, ErrBadAddress
	}
	raw := ci.ServerPublic.Raw32() //nolint:staticcheck // SA1019: no non-deprecated way to get the raw key bytes
	return append([]byte("unic/tailcat/1\x00"), raw[:]...), nil
}

// Forget closes the client of a removed peer.
func (t *Transport) Forget(addr string) {
	t.mu.Lock()
	c := t.clients[addr]
	delete(t.clients, addr)
	t.mu.Unlock()
	if c != nil {
		_ = c.Close()
	}
}

func parseNodeKey(text string) (key.NodePrivate, error) {
	var k key.NodePrivate
	return k, k.UnmarshalText([]byte(text))
}
