package engine

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"net/url"
	"strings"
	"time"

	"unic/internal/config"
	"unic/internal/rendezvous"
)

var errLocked = errors.New("data directory is locked")

// Options configure New. DataDir and the zero values of the rest are enough
// for a first run; hosts normally also pass Settings and HostCapabilities.
type Options struct {
	// DataDir holds the identity and database. It must be private to this
	// instance: New takes an exclusive lock on it and fails with in_use if
	// another instance (or process) holds it.
	DataDir string
	// InboxDir holds resources the engine owns; default DataDir/inbox. The
	// engine never deletes anything in it that it did not create.
	InboxDir string
	// Settings is the complete configuration. The zero value means
	// DefaultSettings. Nothing is read from environment variables or files.
	Settings Settings
	// HostCapabilities describes the platform; by default all clipboard
	// abilities are off.
	HostCapabilities HostCapabilities
	// Resources bounds memory and disk use; zero fields take the defaults.
	Resources ResourcePolicy
	// Logger receives structured diagnostics that never contain clipboard
	// content, addresses, tickets or paths. It must not block. Nil: silent.
	Logger *slog.Logger

	// Network replaces the default tailcat network, for tests and unusual
	// embeddings. Start and Close are called once each.
	Network Network

	// Test seam, reachable from this package's tests only.
	rendezvous *rendezvous.Client
}

// ResourcePolicy bounds what the engine keeps. Zero fields mean the defaults.
type ResourcePolicy struct {
	MaxStoredBytes   int64         // default 8 GiB; must hold Settings.Limits.MaxTotalBytes
	MaxReceived      int           // default 32
	ReceivedTTL      time.Duration // default 30 minutes
	MaxOperations    int           // default 1024
	OperationTTL     time.Duration // default 30 minutes
	MaxSubscriptions int           // default 16
	EventBuffer      int           // default 128, per subscription
}

// Network is the network the engine runs on; the default is tailcat. Listen
// accepts connections from paired devices, Dial reaches one, Addr is this
// device's own address (a secret, like a password). Start brings the network
// up and Close takes it down; Listen may be called again after a suspend.
type Network interface {
	Listen(ctx context.Context) (net.Listener, error)
	Dial(ctx context.Context, addr string) (net.Conn, error)
	Addr() string
	Start() error
	Close() error
}

// DefaultSettings are the settings of a fresh install.
func DefaultSettings() Settings {
	d := config.Default()
	return fromConfig(d)
}

func fromConfig(c config.Config) Settings {
	return Settings{
		DeviceName: c.DeviceName, RendezvousURL: c.RendezvousURL,
		Limits: Limits{MaxFileBytes: int64(c.Limits.MaxFileBytes), MaxTotalBytes: int64(c.Limits.MaxTotalBytes)},
		Sync:   SyncSettings(c.Sync),
	}
}

func (s Settings) toConfig() config.Config {
	return config.Config{
		DeviceName: strings.TrimSpace(s.DeviceName), RendezvousURL: strings.TrimRight(s.RendezvousURL, "/"),
		Limits: config.Limits{MaxFileBytes: config.Size(s.Limits.MaxFileBytes), MaxTotalBytes: config.Size(s.Limits.MaxTotalBytes)},
		Sync:   config.Sync(s.Sync),
	}
}

// validate checks s with the same rules as the configuration file (0002).
func (s Settings) validate() *Error {
	c := s.toConfig()
	if c.RendezvousURL != "" {
		if u, err := url.Parse(c.RendezvousURL); err != nil || (u.Scheme != "https" && u.Scheme != "http") || u.Host == "" {
			return newErr(CodeInvalidArgument, "rendezvous_url: must be an http(s) URL")
		}
	}
	if err := c.Validate(); err != nil {
		// Validate's messages name the field and the rule, never a value that
		// could be a secret.
		return newErr(CodeInvalidArgument, err.Error())
	}
	return nil
}

func (p ResourcePolicy) withDefaults() (ResourcePolicy, *Error) {
	if p.MaxStoredBytes < 0 || p.MaxReceived < 0 || p.ReceivedTTL < 0 || p.MaxOperations < 0 ||
		p.OperationTTL < 0 || p.MaxSubscriptions < 0 || p.EventBuffer < 0 {
		return p, newErr(CodeInvalidArgument, "resource policy values must not be negative")
	}
	def := func(v *int64, d int64) {
		if *v == 0 {
			*v = d
		}
	}
	defi := func(v *int, d int) {
		if *v == 0 {
			*v = d
		}
	}
	def(&p.MaxStoredBytes, 8<<30)
	defi(&p.MaxReceived, 32)
	defi(&p.MaxOperations, 1024)
	defi(&p.MaxSubscriptions, 16)
	defi(&p.EventBuffer, 128)
	if p.ReceivedTTL == 0 {
		p.ReceivedTTL = 30 * time.Minute
	}
	if p.OperationTTL == 0 {
		p.OperationTTL = 30 * time.Minute
	}
	return p, nil
}
