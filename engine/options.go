package engine

import (
	"context"
	"errors"
	"log/slog"
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
	// CacheDir is private storage owned by Engine. Empty: DataDir/inbox/file-cache.
	// The host selects an app-private directory, never a user-selected destination.
	CacheDir string
	// StoreReceivedFile, when set, saves incoming files through the host.
	// Engine supplies a reader valid only during the call. The returned source
	// must have a non-empty persistent reference and must remain readable.
	// Engine never deletes host-managed files, including after a failed commit.
	StoreReceivedFile func(context.Context, ReceivedFile) (PersistentFileSource, error)
	// DeferFileCleanup lets a clipboard host establish startup pins before physical reclamation.
	DeferFileCleanup bool
	// Settings is the complete configuration. The zero value means
	// DefaultSettings. Nothing is read from environment variables or files.
	Settings Settings
	// HostCapabilities describes the platform; by default all clipboard
	// abilities are off.
	HostCapabilities HostCapabilities
	// Resources bounds memory and disk use; zero fields take the defaults.
	Resources ResourcePolicy
	// ResolveFileRef rebuilds the FileSource of a file history kept by
	// reference (see PersistentFileSource). Nil: such files cannot be read
	// after a restart and report source_changed.
	ResolveFileRef func(ref string) FileSource
	// History bounds the persistent local history; zero fields take defaults.
	History HistoryOptions
	// MigrationMode opens an offline import session. Start is forbidden;
	// import bypasses clipboard processing and never evicts existing history.
	MigrationMode bool
	// Logger receives structured diagnostics that never contain clipboard
	// content, addresses, tickets or paths. It must not block. Nil: silent.
	Logger *slog.Logger
	// Diagnostics receives fixed business-stage records outside engine locks.
	Diagnostics func(context.Context, DiagnosticRecord)
	// ProductEvents receives finite completed-action records outside the state mutex.
	// It must return immediately. Nil disables product event collection.
	ProductEvents func(context.Context, ProductEvent)

	// Plugins run in this process, in this order, before the engine is
	// returned: Init and Start happen in New, Close when the engine closes.
	Plugins []PluginInstance
	// LuaPlugins run after Plugins, in this order, under the Lua sandbox.
	LuaPlugins []LuaPluginSpec
	// PluginNotify shows a plugin's short message to the user. It must not
	// block. Nil: messages are dropped.
	PluginNotify func(plugin PluginID, message string)

	// Test seams, reachable from this package's tests only.
	extraBackends    []syncBackend // fake backends for contract tests
	noPersistDefault bool          // processed items default to persist=false
	rendezvous       *rendezvous.Client
}

// ResourcePolicy bounds what the engine keeps. Zero fields mean the defaults.
type ResourcePolicy struct {
	MaxStoredBytes      int64         // default 8 GiB; must hold Settings.Limits.MaxTotalBytes
	MaxReceived         int           // default 32
	ReceivedTTL         time.Duration // default 30 minutes
	MaxOperations       int           // default 1024
	OperationTTL        time.Duration // default 30 minutes
	MaxSubmissions      int           // default 1024; independent of retained send operations
	SubmissionTTL       time.Duration // default 30 minutes from processing completion
	MaxSyncDedupEntries int           // default 100000; committed receives remembered across restarts
	SyncDedupTTL        time.Duration // default 7 days
	MaxSubscriptions    int           // default 16
	EventBuffer         int           // default 128, per subscription
	MaxSyncBackends     int           // default 8, at most 32; sync backend plugins that may be enabled together
}

// DefaultSettings are the settings of a fresh install: everything syncs, up to
// the protocol's limits.
func DefaultSettings() Settings {
	return Settings{
		FileCache: FileCacheSettings{MaxBytes: 500 << 20},
		History:   HistoryCleanupSettings{AutoCleanup: true, RetentionDays: 180, ExcludedTags: []string{}},
		Limits:    Limits{MaxFileBytes: 100 << 20, MaxTotalBytes: wire.MaxTotal},
		Sync:      SyncSettings{Enabled: true, Automatic: true, Files: true, Introductions: true},
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
	if s.History.MaxItems < 0 || s.History.MaxItems > 1_000_000 || s.History.RetentionDays < 0 || s.History.RetentionDays > 36500 {
		return newErr(CodeInvalidArgument, "history: invalid count or retention")
	}
	if _, err := normalizeTags(s.History.ExcludedTags); err != nil {
		return err
	}

	if s.FileCache.MaxBytes < 0 || s.FileCache.MaxBytes > 8<<30 {
		return newErr(CodeInvalidArgument, "file_cache: invalid quota")
	}
	name := strings.TrimSpace(s.DeviceName)
	if len(name) > 64 || strings.ContainsAny(name, "\r\n\x00") {
		return newErr(CodeInvalidArgument, "device_name: at most 64 characters, one line")
	}
	if u := strings.TrimRight(s.RendezvousURL, "/"); u != "" {
		if p, err := url.Parse(u); err != nil || (p.Scheme != "https" && p.Scheme != "http") || p.Host == "" {
			return newErr(CodeInvalidArgument, "rendezvous_url: must be an http(s) URL")
		}
	}
	for _, p := range s.BackendPolicies {
		if p.AutoFileMaxBytes < 0 || p.AutoFileMaxBytes > wire.MaxFileBytes {
			return errf(CodeInvalidArgument, "auto_file_max_bytes: must be between 0 and %d bytes (2GiB)", int64(wire.MaxFileBytes))
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
		p.OperationTTL < 0 || p.MaxSyncDedupEntries < 0 || p.SyncDedupTTL < 0 || p.MaxSubscriptions < 0 || p.EventBuffer < 0 || p.MaxSubmissions < 0 || p.SubmissionTTL < 0 || p.MaxSyncBackends < 0 {
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
	defi(&p.MaxSubmissions, 1024)
	defi(&p.MaxSyncDedupEntries, 100000)
	defi(&p.MaxSubscriptions, 16)
	defi(&p.EventBuffer, 128)
	defi(&p.MaxSyncBackends, 8)
	if p.MaxSyncBackends > 32 {
		return p, newErr(CodeInvalidArgument, "MaxSyncBackends must be between 1 and 32")
	}
	if p.ReceivedTTL == 0 {
		p.ReceivedTTL = 30 * time.Minute
	}
	if p.OperationTTL == 0 {
		p.OperationTTL = 30 * time.Minute
	}
	if p.SyncDedupTTL == 0 {
		p.SyncDedupTTL = 7 * 24 * time.Hour
	}
	if p.SubmissionTTL == 0 {
		p.SubmissionTTL = 30 * time.Minute
	}
	return p, nil
}
