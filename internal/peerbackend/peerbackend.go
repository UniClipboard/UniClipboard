// Package peerbackend is the contract between the engine and a peer-to-peer
// sync backend (builtin.tailcat). It exists so the backend does not import the
// engine and the engine does not import any transport.
package peerbackend

import (
	"context"

	"github.com/uniclipboard/unic/internal/session"
	"github.com/uniclipboard/unic/internal/store"
)

// Env is what the engine gives the peer backend factory.
type Env struct {
	Store  *store.Store
	Config session.Config // everything but Transport
}

// Backend is the peer-to-peer backend as the engine drives it.
// Start and Close are called once each around the engine's life; Addr is
// this device's own address (a secret).
type Backend interface {
	Start() error
	Close() error
	Addr() string
	RunReady(ctx context.Context, ready func()) error
	Connect(ctx context.Context, addr, expect string, tok []byte) (session.Standing, bool, error)
	Connected(deviceID string) bool
	Reconcile()
	NetworkChanged()
	Subscribe() (<-chan session.Event, func())
	Submit(prepare func(peers []string) (session.Job, bool)) bool
}

// Factory builds the backend from the engine's environment.
type Factory func(Env) (Backend, error)
