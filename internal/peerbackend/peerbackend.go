// Package peerbackend is the contract between the engine and a peer-to-peer
// sync backend (builtin.tailcat). It exists so the backend does not import the
// engine and the engine does not import any transport or session layer: the
// types both sides exchange are defined here.
package peerbackend

import (
	"context"
	"errors"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/wire"
)

// Standing is what the host says about a verified device.
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

// Target is an authorized device to keep a connection to.
type Target struct {
	DeviceID string
	Name     string
	Addr     string // a secret
}

// Verified is a device that proved its identity on a connection.
type Verified struct {
	PublicKey []byte
	Name      string // self-reported; never evidence of anything
	Addr      string // its address as reported or dialed; a secret
	Context   []byte // the transport's context binding for Addr, empty if Addr is unusable
	Ticket    bool   // it proved it holds a live pairing ticket (grants nothing)
}

// Host owns devices, trust and pairing tickets. Calls may come from any
// backend goroutine and must not call back into the Engine.
type Host interface {
	// Verify records a verified device (pending unless already known) and its
	// path through this backend, and reports where it stands.
	Verify(v Verified) (Standing, error)
	// Standing reports the current standing of a device, ok=false if unknown.
	Standing(deviceID string) (Standing, bool)
	// Targets lists the devices to keep connected.
	Targets() []Target
	// Pair reports whether proof is a valid PairProof of a live ticket for the
	// transcript t, and uses that ticket up.
	Pair(t [32]byte, proof []byte) bool
	// Active reports a session coming up (nil) or a failed attempt.
	Active(deviceID string, err error)
}

// Receiver stores clips that arrive from other devices.
type Receiver interface {
	// Admit is asked before any byte of a clip is read. It returns false to
	// have the clip read and dropped (no room, say). Otherwise the clip is
	// read into Inbox and handed to Done, which must be called exactly once:
	// with ok=true and the clip (dir is the directory holding its files, owned
	// by the receiver from then on), or with ok=false if it was not delivered.
	Admit(peerID string, m wire.Clip) (Admission, bool)
}

// Admission is the receiver's acceptance of one clip.
type Admission struct {
	Inbox string
	Done  func(snap clip.Snapshot, dir string, ok bool)
}

// Settings are the parts of the user's configuration the backend acts on.
type Settings struct {
	DeviceName    string
	RendezvousURL string
	MaxFileBytes  int64
	MaxTotalBytes int64
	SyncEnabled   bool
}

// DefaultSettings: sync on, the protocol's own limits.
func DefaultSettings() Settings {
	return Settings{MaxFileBytes: wire.MaxFileBytes, MaxTotalBytes: wire.MaxTotal, SyncEnabled: true}
}

// SettingsSource provides the settings in effect, re-read on every use.
type SettingsSource interface{ Current() Settings }

// Event tells that something changed: "peer-up", "peer-down". It carries
// nothing else; whoever listens looks.
type Event struct {
	Kind string
}

// TargetState is how far a clip got towards one device.
type TargetState string

const (
	Sending    TargetState = "sending"
	Sent       TargetState = "sent"
	Failed     TargetState = "failed"
	Superseded TargetState = "superseded"
)

// Job is a clip that a host submits to every connected device. Report and Send
// are called from the backend's goroutines, never with an engine lock held.
type Job struct {
	// Report tells how the clip fares towards peer: Sending, then Sent or
	// Failed; or Superseded if a newer clip replaced it before it started.
	Report func(peer string, st TargetState, err error)
	// Send writes the clip to w; origin is this device's id. ctx ends when the
	// connection does. It reports broken=true when it may have written part of
	// a message, in which case the connection is dropped.
	Send func(ctx context.Context, w *wire.Conn, origin string) (broken bool, err error)
}

// Sentinel errors, so that callers can classify failures without matching text.
var (
	ErrNotRunning = errors.New("engine is not running")
	ErrSelf       = errors.New("that is this device itself")
	// ErrBadAddress means a transport rejected the text as an address.
	ErrBadAddress = errors.New("not a valid tailcat address")
	ErrProtocol   = errors.New("peer speaks another protocol version")
	// ErrIdentity: the device behind the address is not the one expected.
	ErrIdentity = errors.New("address belongs to a different device")
	// ErrBadProof: the peer could not prove the identity it claimed.
	ErrBadProof = errors.New("the device could not prove its identity")
	// ErrNotAllowed: the device is known now but not trusted or authorized
	// for this backend, so no session was made.
	ErrNotAllowed = errors.New("device is not authorized; waiting for the user")
	// ErrSessionClosed is the failure of a clip whose connection ended before
	// or while it was sent.
	ErrSessionClosed = errors.New("connection closed")
)

// Env is what the engine gives the peer backend factory.
type Env struct {
	Dir      string // private instance directory, locked for the engine's life
	Identity *identity.Identity
	Name     string
	Host     Host
	Receiver Receiver
	Logf     func(format string, args ...any)
	Settings SettingsSource
}

// Backend is the peer-to-peer backend as the engine drives it.
// Start and Close are called once each around the engine's life; Addr is
// this device's own address (a secret).
type Backend interface {
	Start() error
	Close() error
	Addr() string
	RunReady(ctx context.Context, ready func()) error
	Connect(ctx context.Context, addr, expect string, tok []byte) (Standing, bool, error)
	Connected(deviceID string) bool
	Reconcile()
	NetworkChanged()
	Subscribe() (<-chan Event, func())
	Submit(prepare func(peers []string) (Job, bool)) bool
}

// Factory builds the backend from the engine's environment.
type Factory func(Env) (Backend, error)
