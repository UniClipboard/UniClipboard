package rpc

// Connection states and finite transport path classifications for peer links.
const (
	LinkStateConnected    = "connected"
	LinkStateConnecting   = "connecting"
	LinkStateDisconnected = "disconnected"

	LinkPathUnknown   = "unknown"
	LinkPathDirect    = "direct"
	LinkPathDERP      = "derp"
	LinkPathPeerRelay = "peer_relay"
)

// PeerLinkStatus is a sanitized runtime link summary for one peer on a backend.
// It never carries secret addresses, raw endpoints, or relay region names.
type PeerLinkStatus struct {
	PeerID        string `json:"peer_id"`
	State         string `json:"state"`
	Path          string `json:"path,omitempty"`
	LatencyMs     int64  `json:"latency_ms,omitempty"`
	LastCheckedMs int64  `json:"last_checked_ms,omitempty"`
}

// Validate checks that PeerLinkStatus uses finite categories and non-negative timings.
func (s PeerLinkStatus) Validate() error {
	if s.PeerID == "" || len(s.PeerID) > 256 {
		return NewError(CodeInvalidArgument)
	}
	switch s.State {
	case LinkStateConnected, LinkStateConnecting, LinkStateDisconnected:
	default:
		return NewError(CodeInvalidArgument)
	}
	switch s.Path {
	case "", LinkPathUnknown, LinkPathDirect, LinkPathDERP, LinkPathPeerRelay:
	default:
		return NewError(CodeInvalidArgument)
	}
	if s.LatencyMs < 0 || s.LastCheckedMs < 0 {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

// ValidLinkPath reports whether p is a known finite non-empty path category.
func ValidLinkPath(p string) bool {
	switch p {
	case LinkPathDirect, LinkPathDERP, LinkPathPeerRelay, LinkPathUnknown:
		return true
	}
	return false
}
