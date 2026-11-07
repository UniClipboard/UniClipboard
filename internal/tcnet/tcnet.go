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

	"github.com/uniclipboard/unic/internal/store"
)

// ErrBadAddress means the text is not a tailcat address.
var ErrBadAddress = errors.New("not a valid tailcat address")

// Port is the virtual TCP port the sync protocol listens on inside the tunnel.
const Port = 7777

type Transport struct {
	st   *store.Store
	logf func(string, ...any)

	srv       *tailcat.Server
	clientKey key.NodePrivate

	mu      sync.Mutex
	clients map[string]*tailcat.Client
	addr    string
}

const (
	metaServerKey = "tc.server.key"
	metaPSK       = "tc.server.psk"
	metaAddr      = "tc.server.addr"
	metaClientKey = "tc.client.key"
)

func New(st *store.Store, logf func(string, ...any)) (*Transport, error) {
	t := &Transport{st: st, logf: logf, clients: map[string]*tailcat.Client{}}

	var err error
	if t.clientKey, err = loadNodeKey(st, metaClientKey); err != nil {
		return nil, err
	}
	srvKey, err := loadNodeKey(st, metaServerKey)
	if err != nil {
		return nil, err
	}
	psk, err := loadPSK(st)
	if err != nil {
		return nil, err
	}
	t.srv = &tailcat.Server{Key: srvKey, PresharedKey: psk, Logf: logf}

	// Pin the DERP region the address was first issued with. Without this a
	// laptop that wakes up on another network would pick another nearest
	// relay and silently change the address its peers have stored.
	if b, err := st.Meta(metaAddr); err != nil {
		return nil, err
	} else if b != nil {
		if ci, err := tailcat.ParseAddr(tailcat.Addr(b)); err == nil && len(ci.Region) == 1 {
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
	addr := string(t.srv.TailcatAddr())
	t.mu.Lock()
	t.addr = addr
	t.mu.Unlock()
	return t.st.SetMeta(metaAddr, []byte(addr))
}

func (t *Transport) Close() error { return t.srv.Close() }

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
		return nil, fmt.Errorf("%w: %v", ErrBadAddress, err)
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

func loadNodeKey(st *store.Store, name string) (key.NodePrivate, error) {
	var k key.NodePrivate
	b, err := st.Meta(name)
	if err != nil {
		return k, err
	}
	if b != nil {
		return k, k.UnmarshalText(b)
	}
	k = key.NewNode()
	b, err = k.MarshalText()
	if err != nil {
		return k, err
	}
	return k, st.SetMeta(name, b)
}

func loadPSK(st *store.Store) (tailcat.PresharedKey, error) {
	var p tailcat.PresharedKey
	b, err := st.Meta(metaPSK)
	if err != nil {
		return p, err
	}
	if b != nil {
		return p, p.UnmarshalText(b)
	}
	p = tailcat.NewPresharedKey()
	b, err = p.MarshalText()
	if err != nil {
		return p, err
	}
	return p, st.SetMeta(metaPSK, b)
}
