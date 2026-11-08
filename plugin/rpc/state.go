package rpc

import (
	"bytes"
	"encoding/json"
	"unicode/utf8"
)

// Peer-sessions backend binding and state snapshot (sync-backend-contract §1–2).
const (
	MethodBind       = "backend.bind"
	MethodApplyState = "backend.apply_state"
	PurposeSyncState = "sync_state"

	MaxStateBytes    = 64 << 20
	MaxStatePeers    = 4096
	MaxStateRejected = 4096
	WireVersion      = 2
)

// Bind configures one backend session. It never connects.
type Bind struct {
	BindingID     string `json:"binding_id"`
	SelfID        string `json:"self_id"`
	SelfName      string `json:"self_name"`
	WireVersion   int    `json:"wire_version"`
	BackendID     string `json:"backend_id"`
	TransportKind string `json:"transport_kind"`
}

func (b Bind) Validate() error {
	if !hex64Re.MatchString(b.BindingID) || !validConn(b.SelfID) || !validConn(b.BackendID) || len(b.SelfName) > MaxPeerNameBytes ||
		!utf8.ValidString(b.SelfName) || b.WireVersion != WireVersion || b.TransportKind != "peer_sessions" {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

// StatePeer is a trusted device the backend may connect to. Address is secret.
type StatePeer struct {
	ID            string `json:"id"`
	Name          string `json:"name"`
	Address       string `json:"address"`
	TrustRevision U64    `json:"trust_revision"`
}

// StateRejected is an identity the user declined; it must not be treated as unknown.
type StateRejected struct {
	ID              string `json:"id"`
	RequestRevision U64    `json:"request_revision"`
}

type StateLimits struct {
	MaxFileBytes  U64 `json:"max_file_bytes"`
	MaxTotalBytes U64 `json:"max_total_bytes"`
}

// BackendPolicy is the per-backend type policy.
type BackendPolicy struct {
	Text             bool `json:"text"`
	Images           bool `json:"images"`
	Files            bool `json:"files"`
	AutoFileMaxBytes U64  `json:"auto_file_max_bytes"`
}

// SyncState is the complete state one backend instance may act on.
type SyncState struct {
	BackendID         string          `json:"backend_id"`
	BindingID         string          `json:"binding_id"`
	Revision          U64             `json:"revision"`
	Runtime           string          `json:"runtime"` // active or suspended
	DeviceName        string          `json:"device_name"`
	SyncEnabled       bool            `json:"sync_enabled"`
	Policy            BackendPolicy   `json:"policy"`
	Limits            StateLimits     `json:"limits"`
	NetworkGeneration U64             `json:"network_generation"`
	Peers             []StatePeer     `json:"peers"`
	Rejected          []StateRejected `json:"rejected"`
}

// Validate checks bounds and that no identity is both trusted and rejected
// or listed twice; nothing is truncated to fit.
func (s SyncState) Validate() error {
	bad := NewError(CodeInvalidArgument)
	if !validConn(s.BackendID) || !hex64Re.MatchString(s.BindingID) || (s.Runtime != "active" && s.Runtime != "suspended") ||
		len(s.DeviceName) > MaxPeerNameBytes || !utf8.ValidString(s.DeviceName) || s.Peers == nil || s.Rejected == nil {
		return bad
	}
	if len(s.Peers) > MaxStatePeers || len(s.Rejected) > MaxStateRejected {
		return NewError(CodeExhausted)
	}
	seen := make(map[string]bool, len(s.Peers)+len(s.Rejected))
	for _, p := range s.Peers {
		if !validConn(p.ID) || seen[p.ID] || len(p.Name) > MaxPeerNameBytes || !utf8.ValidString(p.Name) ||
			p.Address == "" || len(p.Address) > MaxPeerAddress || !utf8.ValidString(p.Address) {
			return bad
		}
		seen[p.ID] = true
	}
	for _, r := range s.Rejected {
		if !validConn(r.ID) || seen[r.ID] {
			return bad
		}
		seen[r.ID] = true
	}
	return nil
}

// Canonical is the byte form compared for idempotent re-delivery.
func (s SyncState) Canonical() []byte {
	b, _ := json.Marshal(s)
	return b
}

func (s SyncState) Equal(o SyncState) bool { return bytes.Equal(s.Canonical(), o.Canonical()) }

// ApplyState delivers a snapshot as a sync_state stream rooted at the request.
type ApplyState struct {
	Revision U64        `json:"revision"`
	Snapshot StreamDesc `json:"snapshot"`
}

func (a ApplyState) Validate() error {
	if a.Revision == 0 || a.Snapshot.Purpose != PurposeSyncState || a.Snapshot.Size == 0 || a.Snapshot.Size > MaxStateBytes {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

// Revision is the apply_state result: the revision now in force.
type Revision struct {
	Revision U64 `json:"revision"`
}
