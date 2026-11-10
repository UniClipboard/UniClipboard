package tcnet

import (
	"net"
	"net/netip"
	"time"

	"tailscale.com/types/key"
)

// Path reduces a passive server snapshot before it leaves this boundary. The
// pinned tailcat client has no public status API; outgoing sessions are unknown.
// Server.Status is never called before Start succeeds or concurrently with Close.
func (t *Transport) Path(conn net.Conn, outgoing bool) string {
	if outgoing {
		return "unknown"
	}
	return t.PathSnapshot()(conn, outgoing)
}

// PathSnapshot reads status once per collection, then retains only a private
// virtual-IP/category mapping. Full status, node keys and raw endpoints never
// leave this function or enter the observer. Prefix collisions remain unknown.
func (t *Transport) PathSnapshot() func(net.Conn, bool) string {
	unknown := func(net.Conn, bool) string { return "unknown" }
	t.statusMu.Lock()
	defer t.statusMu.Unlock()
	if !t.statusReady || t.statusClosed {
		return unknown
	}
	status := t.srv.Status()
	if status == nil {
		return unknown
	}
	now := time.Now()
	paths := make(map[netip.Addr]string, len(status.Peer))
	for node, p := range status.Peer {
		ip := tunnelIP(node)
		if _, duplicate := paths[ip]; duplicate {
			paths[ip] = "unknown"
			continue
		}
		path := "unknown"
		if p != nil && p.Active && !p.LastWrite.IsZero() && now.Sub(p.LastWrite) >= 0 && now.Sub(p.LastWrite) <= 30*time.Second {
			if addr, err := netip.ParseAddrPort(p.CurAddr); err == nil && addr.IsValid() {
				path = "direct"
			} else if p.PeerRelay != "" {
				path = "peer_relay"
			} else if p.CurAddr == "" && p.Relay != "" {
				path = "derp"
			}
		}
		paths[ip] = path
	}
	return func(conn net.Conn, outgoing bool) string {
		if outgoing {
			return "unknown"
		}
		host, _, err := net.SplitHostPort(conn.RemoteAddr().String())
		if err != nil {
			return "unknown"
		}
		ip, err := netip.ParseAddr(host)
		if err != nil {
			return "unknown"
		}
		if path, ok := paths[ip]; ok {
			return path
		}
		return "unknown"
	}
}

// v0.7.0 assigns ULA /48 + the first 80 node-key bits to netstack peers.
// Status lacks TailscaleIPs in this revision. This version-bound conversion
// matches the accepted socket's virtual IP without using private library state.
// Ambiguous prefix collisions are rejected by Path rather than guessed.
func tunnelIP(k key.NodePublic) netip.Addr {
	a := [16]byte{0xfd, 0x7a, 0x11, 0x5c, 0xa1, 0xe0}
	raw := k.Raw32() //nolint:staticcheck // pinned upstream address representation
	copy(a[6:], raw[:10])
	return netip.AddrFrom16(a)
}
