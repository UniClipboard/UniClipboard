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
	"errors"
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

	startCtx    context.Context
	startCancel context.CancelFunc
	srv         *tailcat.Server
	clientKey   key.NodePrivate

	mu      sync.Mutex
	clients map[string]*peerClient
	addr    string
	creds   credentials
}

type peerClient struct {
	client *tailcat.Client
	users  int // in-flight dials and open connections, guarded by Transport.mu
}

// New prepares the transport with credentials kept in dir, the backend's own
// private instance directory. The three keys are made together and published
// in one atomic write, so a failure leaves either all of them or none. A
// credentials file that exists but cannot be trusted is an error: new keys are
// never generated in its place, since that would silently change the address
// every paired device has stored.
func New(dir string, logf func(string, ...any)) (*Transport, error) {
	startCtx, startCancel := context.WithCancel(context.Background())
	t := &Transport{dir: dir, logf: logf, startCtx: startCtx, startCancel: startCancel, clients: map[string]*peerClient{}}
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
// DERP region discovery runs outside statusMu with a cancellable context so
// Close can abort startup promptly without racing tailcat.Server.Start/Close.
func (t *Transport) Start() error {
	t.statusMu.Lock()
	if t.statusClosed {
		t.statusMu.Unlock()
		return errors.New("start tailcat: transport closed")
	}
	needRegion := t.srv.Region == nil
	t.statusMu.Unlock()

	var expanded *tailcat.ConnInfo
	if needRegion {
		ci := &tailcat.ConnInfo{RegionID: -1}
		if err := ci.Expand(t.startCtx, tailcat.ExpandForServer); err != nil {
			return fmt.Errorf("start tailcat: %w", err)
		}
		expanded = ci
	}

	t.statusMu.Lock()
	if t.statusClosed {
		t.statusMu.Unlock()
		return errors.New("start tailcat: transport closed")
	}
	if t.srv.Region == nil && expanded != nil && len(expanded.Region) > 0 {
		t.srv.Region = expanded.Region[0]
	}
	if err := t.srv.Start(); err != nil {
		t.statusMu.Unlock()
		return fmt.Errorf("start tailcat: %w", err)
	}
	addr := string(t.srv.TailcatAddr())
	t.statusReady = true
	t.statusMu.Unlock()

	t.mu.Lock()
	t.addr = addr
	if t.creds.Addr == addr {
		t.mu.Unlock()
		return nil
	}
	c := t.creds
	c.Addr = addr
	if err := saveCredentials(t.dir, c); err != nil {
		t.mu.Unlock()
		return err
	}
	t.creds = c
	t.mu.Unlock()
	return nil
}

func (t *Transport) Close() error {
	t.startCancel()
	t.statusMu.Lock()
	defer t.statusMu.Unlock()
	t.statusClosed = true
	t.statusReady = false
	return t.srv.Close()
}

func (t *Transport) Listen(ctx context.Context) (net.Listener, error) {
	t.statusMu.Lock()
	defer t.statusMu.Unlock()
	if t.statusClosed || !t.statusReady {
		return nil, errors.New("listen tailcat: transport not started or closed")
	}
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
		c = &peerClient{client: &tailcat.Client{Server: tailcat.Addr(addr), Key: t.clientKey, Logf: t.logf}}
		t.clients[addr] = c
	}
	c.users++
	t.mu.Unlock()
	release := func() {
		t.mu.Lock()
		defer t.mu.Unlock()
		c.users--
		if c.users == 0 {
			// A peer restart can invalidate registration and WireGuard
			// state. The next dial starts a clean stack with the same key.
			_ = c.client.Close()
			delete(t.clients, addr)
		}
	}
	conn, err := c.client.DialTCPPort(ctx, Port)
	if err != nil {
		release()
		return nil, err
	}
	return &clientConn{Conn: conn, client: c.client, release: release}, nil
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

func parseNodeKey(text string) (key.NodePrivate, error) {
	var k key.NodePrivate
	return k, k.UnmarshalText([]byte(text))
}
