// Package peerbackend holds the small types the engine's peer-session path
// shares: what a verified device looks like, what a receive admission is, and
// the sentinel errors pairing classifies. Backends themselves are plugins
// (plugin.PeerBackend); this package defines no backend interface.
package peerbackend

import (
	"errors"

	"github.com/uniclipboard/unic/internal/clip"
)

// Standing is what the engine says about a verified device.
type Standing struct {
	DeviceID string
	Name     string // the user's name for it, else what it reports
	// Allowed: the device is trusted and its path through this backend is
	// authorized. Only allowed devices get a session.
	Allowed bool
	// SyncEnabled: the path may carry clips now (the user can pause it
	// without dropping the session).
	SyncEnabled bool
}

// Verified is a device that proved its identity on a connection.
type Verified struct {
	PublicKey []byte
	Name      string // self-reported; never evidence of anything
	Addr      string // its address as reported or dialed; a secret
	Context   []byte // the transport's context binding for Addr, empty if Addr is unusable
	Ticket    bool   // it proved it holds a live pairing ticket (grants nothing)
}

// Admission is the receiver's acceptance of one clip.
type Admission struct {
	Inbox string
	Done  func(snap clip.Snapshot, dir string, ok bool)
}

// Sentinel errors, so that callers can classify failures without matching text.
var (
	ErrNotRunning = errors.New("engine is not running")
	ErrSelf       = errors.New("that is this device itself")
	// ErrBadAddress means a transport rejected the text as an address.
	ErrBadAddress = errors.New("not a valid peer address")
	ErrProtocol   = errors.New("peer speaks another protocol version")
	// ErrIdentity: the device behind the address is not the one expected.
	ErrIdentity = errors.New("address belongs to a different device")
	// ErrBadProof: the peer could not prove the identity it claimed.
	ErrBadProof = errors.New("the device could not prove its identity")
	// ErrSessionClosed is the failure of a clip whose connection ended before
	// or while it was sent.
	ErrSessionClosed = errors.New("connection closed")
)
