package engine

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"net/url"
	"strings"
	"time"

	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/wire"
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

// DefaultSettings are the settings of a fresh install: everything syncs, up to
// the protocol's limits.
func DefaultSettings() Settings {
	return Settings{
		Limits: Limits{MaxFileBytes: wire.MaxFileBytes, MaxTotalBytes: wire.MaxTotal},
		Sync:   SyncSettings{Enabled: true, Text: true, Images: true, Files: true},
	}
}

// Validate checks s. The messages name the setting and the rule, never a value
// that could be a secret, and use the names of the configuration file keys.
// Hosts that read settings from a file use it to check them.
func (s Settings) Validate() error {
	if err := s.validate(); err != nil {
		return err
	}
	return nil
}

func (s Settings) validate() *Error {
	name := strings.TrimSpace(s.DeviceName)
	if len(name) > 64 || strings.ContainsAny(name, "\r\n\x00") {
		return newErr(CodeInvalidArgument, "device_name: at most 64 characters, one line")
	}
	if u := strings.TrimRight(s.RendezvousURL, "/"); u != "" {
		if p, err := url.Parse(u); err != nil || (p.Scheme != "https" && p.Scheme != "http") || p.Host == "" {
			return newErr(CodeInvalidArgument, "rendezvous_url: must be an http(s) URL")
		}
	}
	if f := s.Limits.MaxFileBytes; f <= 0 || f > wire.MaxFileBytes {
		return errf(CodeInvalidArgument, "limits.max_file_bytes: must be between 1 byte and %d bytes (2GiB)", int64(wire.MaxFileBytes))
	}
	if t := s.Limits.MaxTotalBytes; t <= 0 || t > wire.MaxTotal {
		return errf(CodeInvalidArgument, "limits.max_total_bytes: must be between 1 byte and %d bytes (4GiB)", int64(wire.MaxTotal))
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
