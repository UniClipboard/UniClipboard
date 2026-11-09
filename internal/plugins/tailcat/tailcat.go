// Package tailcat is the built-in peer-to-peer sync backend
// (builtin.tailcat). It owns the tailcat transport and the session layer; the
// engine knows neither. The host assembles it explicitly and hands the
// factory to the engine (engine.Options.Peer): nothing registers itself.
package tailcat

import (
	"errors"

	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/internal/session"
	"github.com/uniclipboard/unic/internal/tcnet"
)

// Transport is what the backend runs on: the session transport plus its
// lifecycle. tcnet.Transport is the production one; tests inject their own.
type Transport interface {
	session.Transport
	// Start brings the network up, Close takes it down. Listen may be called
	// again after a suspend.
	Start() error
	Close() error
}

// Backend is the session layer on a transport.
type Backend struct {
	*session.Engine
	tr Transport
}

func (b *Backend) Start() error { return b.tr.Start() }
func (b *Backend) Close() error { return b.tr.Close() }
func (b *Backend) Addr() string { return b.tr.Addr() }

// Factory builds the production backend (tailcat transport).
func Factory() peerbackend.Factory { return FactoryOn(nil) }

// FactoryOn builds the backend on tr; nil means the tailcat transport. It is
// how tests supply loopback or fault-injecting networks.
func FactoryOn(tr Transport) peerbackend.Factory {
	return func(env peerbackend.Env) (peerbackend.Backend, error) {
		if tr == nil {
			t, err := tcnet.New(env.Dir, func(string, ...any) {}) // tailcat's logs may name addresses
			if err != nil {
				if errors.Is(err, tcnet.ErrBadCredentials) {
					return nil, err // a damaged file is reported, never replaced
				}
				return nil, errors.New("cannot prepare the network")
			}
			tr = t
		}
		cfg := session.Config{Identity: env.Identity, Name: env.Name, Host: env.Host, Transport: tr,
			Logf: env.Logf, Settings: env.Settings, Receiver: env.Receiver}
		if cfg.Logf == nil {
			cfg.Logf = func(string, ...any) {} // session logs name devices; never default to the global logger
		}
		return &Backend{Engine: session.New(cfg), tr: tr}, nil
	}
}
