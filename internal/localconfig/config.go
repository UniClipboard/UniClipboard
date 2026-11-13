// Package config loads unic's TOML configuration file and keeps it current:
// the file is watched and every valid save takes effect at once. See
// docs/decisions/proposed/0002-config-file.md.
package config

import (
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/plugins/builtinclassify"
	"github.com/uniclipboard/unic/internal/plugins/builtintext"
)

// Hard caps of the wire protocol; limits in the file cannot exceed them.
const (
	HardMaxFile  = 2 << 30
	HardMaxTotal = 4 << 30
	// HardMaxHistory is the engine's default total storage budget, which
	// history's own budget must fit in.
	HardMaxHistory = 8 << 30
)

// Size is a byte count written as an integer or as a string like "512MB".
type Size int64

func (s *Size) UnmarshalTOML(v any) error {
	switch v := v.(type) {
	case int64:
		*s = Size(v)
		return nil
	case string:
		n, err := ParseSize(v)
		*s = Size(n)
		return err
	}
	return errors.New(`must be a number of bytes or a string like "512MB"`)
}

// ParseSize parses "123", "64KB", "512MB", "2GiB" (units are powers of 1024).
func ParseSize(str string) (int64, error) {
	t := strings.ToUpper(strings.ReplaceAll(strings.TrimSpace(str), " ", ""))
	mult := int64(1)
	for _, u := range []struct {
		suf string
		m   int64
	}{{"KIB", 1 << 10}, {"MIB", 1 << 20}, {"GIB", 1 << 30}, {"KB", 1 << 10}, {"MB", 1 << 20}, {"GB", 1 << 30}, {"B", 1}} {
		if strings.HasSuffix(t, u.suf) {
			mult, t = u.m, strings.TrimSuffix(t, u.suf)
			break
		}
	}
	n, err := strconv.ParseInt(t, 10, 64)
	if err != nil || n < 0 || n > (1<<62)/mult {
		return 0, fmt.Errorf("invalid size %q", str)
	}
	return n * mult, nil
}

// Config is the effective configuration.
type Config struct {
	Desktop       Desktop                 `toml:"desktop" json:"desktop"`
	Logging       observability.Logging   `toml:"logging" json:"logging"`
	Telemetry     observability.Telemetry `toml:"telemetry" json:"telemetry"`
	DeviceName    string                  `toml:"device_name" json:"device_name"`
	RendezvousURL string                  `toml:"rendezvous_url" json:"rendezvous_url"`
	Inbox         string                  `toml:"inbox" json:"inbox"`
	Limits        Limits                  `toml:"limits" json:"limits"`
	Sync          Sync                    `toml:"sync" json:"sync"`
	Plugins       Plugins                 `toml:"plugins" json:"plugins"`
	History       History                 `toml:"history" json:"history"`
	FileCache     FileCache               `toml:"file_cache" json:"file_cache"`
}

// Desktop configures the graphical launcher. The service only stores these values.
type Desktop struct {
	// TagColors maps SHA-256 tag identifiers to local presentation colors. It stores no tag names.
	TagColors          map[string]string `toml:"tag_colors,omitempty" json:"tag_colors,omitempty"`
	Language           string            `toml:"language" json:"language"`
	QuickPanelShortcut string            `toml:"quick_panel_shortcut" json:"quick_panel_shortcut"`
	Mode               string            `toml:"mode" json:"mode"`
	LaunchAtLogin      bool              `toml:"launch_at_login" json:"launch_at_login"`
	StartInBackground  bool              `toml:"start_in_background" json:"start_in_background"`
}

// History configures live cleanup and the startup storage budget.
type History struct {
	AutoCleanup   bool     `toml:"auto_cleanup" json:"auto_cleanup"`
	RetentionDays int      `toml:"retention_days" json:"retention_days"`
	MaxItems      int      `toml:"max_items" json:"max_items"`
	ExcludedTags  []string `toml:"excluded_tags" json:"excluded_tags"`
	MaxBytes      Size     `toml:"max_bytes" json:"max_bytes"`
}

type Limits struct {
	MaxFileBytes  Size `toml:"max_file_bytes" json:"max_file_bytes"`
	MaxTotalBytes Size `toml:"max_total_bytes" json:"max_total_bytes"`
}

// Sync is the one global switch for content sync.
type FileCache struct {
	MaxBytes Size `toml:"max_bytes" json:"max_bytes"`
}

type DirectorySync struct {
	Automatic bool `toml:"automatic" json:"automatic"`
}

type Sync struct {
	Directories DirectorySync `toml:"directories" json:"directories"`
	Automatic   bool          `toml:"automatic" json:"automatic"`
	OnRestore   bool          `toml:"on_restore" json:"on_restore"`
	Files       bool          `toml:"files" json:"files"`
	Enabled     bool          `toml:"enabled" json:"enabled"`
	// Introductions: devices tell each other which others they vouch for; the
	// result is only a list of candidates (see docs/architecture/identity-and-trust.md).
	Introductions bool `toml:"introductions" json:"introductions"`
}

// Plugins holds per-plugin-instance settings for the plugins this build
// ships: builtin.tailcat sync policy and built-in processor switches.
type Plugins struct {
	Instances map[string]Instance `toml:"instances" json:"instances,omitempty"`
	// Lua lists the installed Lua plugins to run, in processing order. Each
	// is a directory under $UNIC_HOME/plugins/lua/ with a spec.toml.
	// Changing the script text takes effect at once; changing this list
	// waits for a restart.
	Lua []string `toml:"lua" json:"lua,omitempty"`
	// OnError is what a failed processor means for syncing, for every plugin
	// that does not set its own: "continue" (default) or "block_sync".
	OnError string `toml:"on_error" json:"on_error,omitempty"`
}

type Instance struct {
	Enabled *bool       `toml:"enabled" json:"enabled,omitempty"` // omitted: on
	Sync    BackendSync `toml:"sync" json:"sync"`
	// OnError overrides [plugins] on_error for this plugin.
	OnError string `toml:"on_error" json:"on_error,omitempty"`
	// Config is the plugin's own settings (Lua plugins). They live here, not
	// in the plugin directory, so installing or upgrading a plugin never
	// replaces what the user chose.
	Config map[string]any `toml:"config" json:"config,omitempty"`
}

// ProcessorSettings is what the host passes a processor plugin: the effective
// on_error policy and its own config.
func (c Config) ProcessorSettings(id string) (onError string, cfg map[string]any) {
	in := c.Plugins.Instances[id]
	switch {
	case in.OnError != "":
		onError = in.OnError
	case c.Plugins.OnError != "":
		onError = c.Plugins.OnError
	default:
		onError = "continue"
	}
	return onError, in.Config
}

// processorSettingsKey is everything of ProcessorSettings that the engine
// fixes at startup, for the plugins that run.
func (c Config) processorSettingsKey() map[string]Instance {
	out := map[string]Instance{}
	ids := append([]string{builtinclassify.ID, builtintext.ID}, c.Plugins.Lua...)
	for _, id := range ids {
		oe, cfg := c.ProcessorSettings(id)
		out[id] = Instance{OnError: oe, Config: cfg}
	}
	return out
}

// PluginEnabled reports whether a shipped plugin is switched on.
func (c Config) PluginEnabled(id string) bool {
	in, ok := c.Plugins.Instances[id]
	return !ok || in.Enabled == nil || *in.Enabled
}

// BackendSync is one backend's standard sync policy. Omitted booleans mean
// true; AutoFileMaxBytes 0 means no extra threshold for automatic sends.
type BackendSync struct {
	Text             *bool `toml:"text" json:"text,omitempty"`
	Images           *bool `toml:"images" json:"images,omitempty"`
	Files            *bool `toml:"files" json:"files,omitempty"`
	AutoFileMaxBytes Size  `toml:"auto_file_max_bytes" json:"auto_file_max_bytes"`
}

func (b BackendSync) policy() engine.BackendSyncPolicy {
	on := func(p *bool) bool { return p == nil || *p }
	return engine.BackendSyncPolicy{Text: on(b.Text), Images: on(b.Images), Files: on(b.Files), AutoFileMaxBytes: int64(b.AutoFileMaxBytes)}
}

// Default returns the configuration used for keys the file leaves out.
func Default() Config {
	shortcut := "Ctrl+Alt+V"
	if runtime.GOOS == "darwin" {
		shortcut = "Cmd+Ctrl+V"
	}
	return Config{
		Desktop: Desktop{Language: "zh-CN", Mode: "normal", QuickPanelShortcut: shortcut},
		Logging: observability.Default().Logging, Telemetry: observability.Default().Telemetry,
		Limits:    Limits{MaxFileBytes: 100 << 20, MaxTotalBytes: HardMaxTotal},
		Sync:      Sync{Enabled: true, Automatic: true, Files: true, Introductions: true},
		History:   History{AutoCleanup: true, RetentionDays: 180, MaxBytes: 4 << 30, ExcludedTags: []string{}},
		FileCache: FileCache{MaxBytes: 500 << 20},
	}
}

// Parse decodes and validates a config file's content. Missing keys keep
// their defaults; unknown keys are an error.
func Parse(data []byte) (Config, error) {
	c := Default()
	md, err := toml.Decode(string(data), &c)
	if err != nil {
		return Config{}, err
	}
	if un := md.Undecoded(); len(un) > 0 {
		keys := make([]string, len(un))
		for i, k := range un {
			keys[i] = k.String()
		}
		return Config{}, fmt.Errorf("unknown key(s): %s", strings.Join(keys, ", "))
	}
	return c, c.Validate()
}

// Settings is the configuration as engine settings.
func (c Config) Settings() engine.Settings {
	return engine.Settings{
		DeviceName: c.DeviceName, RendezvousURL: c.RendezvousURL,
		History:         engine.HistoryCleanupSettings{AutoCleanup: c.History.AutoCleanup, RetentionDays: c.History.RetentionDays, MaxItems: c.History.MaxItems, ExcludedTags: append([]string{}, c.History.ExcludedTags...)},
		FileCache:       engine.FileCacheSettings{MaxBytes: int64(c.FileCache.MaxBytes)},
		Limits:          engine.Limits{MaxFileBytes: int64(c.Limits.MaxFileBytes), MaxTotalBytes: int64(c.Limits.MaxTotalBytes)},
		Sync:            engine.SyncSettings{Directories: engine.DirectorySyncSettings{Automatic: c.Sync.Directories.Automatic}, Enabled: c.Sync.Enabled, Automatic: c.Sync.Automatic, OnRestore: c.Sync.OnRestore, Files: c.Sync.Files, Introductions: c.Sync.Introductions},
		BackendPolicies: c.backendPolicies(),
	}
}

// backendPolicies is the complete policy of every backend this build ships.
func (c Config) backendPolicies() map[engine.PluginID]engine.BackendSyncPolicy {
	out := map[engine.PluginID]engine.BackendSyncPolicy{engine.BackendTailcat: engine.DefaultBackendPolicy()}
	if in, ok := c.Plugins.Instances[string(engine.BackendTailcat)]; ok {
		out[engine.BackendTailcat] = in.Sync.policy()
	}
	return out
}

// Validate checks values; the engine's rules apply to everything the engine
// uses, so the file and the SDK cannot disagree. Messages never include the
// offending values of fields that could carry secrets.
func (c Config) Validate() error {
	switch c.Desktop.Language {
	case "", "zh-CN", "en", "ja", "pt-BR", "ru":
	default:
		return errors.New("desktop.language: unsupported language")
	}
	if len(c.Desktop.TagColors) > 4096 {
		return errors.New("desktop.tag_colors: at most 4096 overrides")
	}
	for id, color := range c.Desktop.TagColors {
		key, err := hex.DecodeString(id)
		if err != nil || len(key) != 32 {
			return errors.New("desktop.tag_colors: invalid tag identifier")
		}
		switch color {
		case "orange", "blue", "green", "purple", "gray":
		default:
			if len(color) != 7 || color[0] != '#' {
				return errors.New("desktop.tag_colors: invalid color")
			}
			if _, err := hex.DecodeString(color[1:]); err != nil {
				return errors.New("desktop.tag_colors: invalid color")
			}
		}
	}
	switch c.Desktop.QuickPanelShortcut {
	case "", "Ctrl+`", "CmdOrCtrl+Shift+Space", "Ctrl+Alt+V", "Cmd+Ctrl+V":
	default:
		return errors.New("desktop.quick_panel_shortcut: unsupported shortcut")
	}
	if c.Desktop.Mode != "normal" && c.Desktop.Mode != "lightweight" {
		return errors.New(`desktop.mode: must be "normal" or "lightweight"`)
	}
	if err := c.Observability().Validate(); err != nil {
		return err
	}
	if c.FileCache.MaxBytes <= 0 || c.FileCache.MaxBytes > HardMaxHistory {
		return errors.New("file_cache: max_bytes must be between 1 byte and 8GiB")
	}
	if c.Inbox != "" && !filepath.IsAbs(c.Inbox) {
		return errors.New("inbox: must be an absolute path")
	}
	if !validOnError(c.Plugins.OnError) {
		return errors.New(`plugins.on_error: must be "continue" or "block_sync"`)
	}
	lua := map[string]bool{}
	for _, id := range c.Plugins.Lua {
		lua[id] = true
	}
	for id, in := range c.Plugins.Instances {
		if !validOnError(in.OnError) {
			return fmt.Errorf(`plugins.instances.%q.on_error: must be "continue" or "block_sync"`, id)
		}
		if in.Config != nil && !lua[id] {
			return fmt.Errorf("plugins.instances.%q: this plugin takes no config", id)
		}
		switch {
		case lua[id]:
			if in.Enabled != nil || in.Sync != (BackendSync{}) {
				return fmt.Errorf("plugins.instances.%q: a Lua plugin runs when it is listed in plugins.lua; only on_error and config apply", id)
			}
			continue
		}
		switch id {
		case string(engine.BackendTailcat):
			if in.OnError != "" {
				return fmt.Errorf("plugins.instances.%q: a sync backend is not a processor", id)
			}
			if in.Enabled != nil {
				return fmt.Errorf("plugins.instances.%q: this plugin cannot be switched off yet", id)
			}
		case builtintext.ID, builtinclassify.ID:
			if in.Sync != (BackendSync{}) {
				return fmt.Errorf("plugins.instances.%q: this plugin has no sync settings", id)
			}
		default:
			return fmt.Errorf("plugins.instances: %q is not an installed plugin", id)
		}
	}
	seen := map[string]bool{}
	for _, id := range c.Plugins.Lua {
		if id == "" || len(id) > 128 || seen[id] {
			return errors.New("plugins.lua: ids must be non-empty and unique")
		}
		seen[id] = true
	}
	h := c.History
	switch {
	case h.MaxItems < 0 || h.MaxItems > 1_000_000:
		return errors.New("history.max_items: must be between 0 and 1000000")
	case h.MaxBytes <= 0 || h.MaxBytes > HardMaxHistory:
		return errors.New("history.max_bytes: must be between 1 byte and 8GiB")
	case h.RetentionDays < 0 || h.RetentionDays > 36500:
		return errors.New("history.retention_days: must be between 0 and 36500")
	}
	return c.Settings().Validate()
}

// HistoryOptions is the history section as engine options.
func (c Config) HistoryOptions() engine.HistoryOptions {
	return engine.HistoryOptions{MaxBytes: int64(c.History.MaxBytes)}
}

// Template is written when no config file exists. Everything is commented
// out, so the file starts as a no-op that documents the options.
const Template = `# unic configuration. Saving this file applies it at once.
# A file with an error is ignored (the previous settings stay) until fixed;
# run ` + "`unic config`" + ` to see the current settings and any error.

# Desktop launcher settings. Mode and background visibility apply on next launch.
# Login registration is applied by a running normal desktop session.
# [desktop]
# language = "zh-CN"  # "zh-CN", "en", "ja", "pt-BR", "ru"; applies immediately
# mode = "normal"  # "normal" or "lightweight"
# launch_at_login = false
# start_in_background = false
# quick_panel_shortcut = "Ctrl+Alt+V"
# macOS default: "Cmd+Ctrl+V"

# Logging and PostHog/OTLP upload. All remote signals are on by default.
# Set telemetry.enabled = false to stop all uploads. Signal switches also hot reload.
# [logging]
# level = "info"
# [telemetry]
# enabled = true
# endpoint = "https://collector.example.com"  # changing endpoint needs restart
# [telemetry.logs]
# enabled = true
# level = "warn"
# [telemetry.metrics]
# enabled = true
# [telemetry.analytics]
# enabled = true

# Name shown to your other devices. Default: this computer's host name.
# Takes effect on the next connection to each device.
# device_name = "my-laptop"

# Pairing code service. Default: $UNIC_RENDEZVOUS_URL, else the built-in one.
# rendezvous_url = "https://rendezvous.example.com"

# Where received files are placed (absolute path). Needs a restart of unicd.
# inbox = "/Users/me/unic-inbox"

[limits]
# Sizes are bytes, or strings like "512MB". Applies to transfers that start
# after the change. Cannot exceed 2GiB per file / 4GiB per clipboard.
# max_file_bytes = "100MB"
# max_total_bytes = "4GiB"

[history]
# Unified automatic cleanup. Rules apply without restart. The oldest entries go
# first. Only max_bytes needs a restart. Files you copy from disk are remembered by path, not copied, so they
# do not count towards max_bytes (but stop being readable if they are moved).
# auto_cleanup = true
# retention_days = 180  # 0: no age limit
# max_items = 0         # 0: no count limit
# excluded_tags = []    # any exact matching tag prevents automatic deletion
# max_bytes = "4GiB"

[file_cache]
# Applies ONLY to the default managed receive directory.
# Custom received files are user-managed and are never deleted by cleanup.
# max_bytes = "500MB"
# Capacity pressure uses unified history cleanup; excluded tags remain protected.
# When cleanup is off or cannot reclaim enough space, new files are rejected.

[sync]
# automatic = true    # new clipboard copies; local history remains enabled
# on_restore = false  # send only after successful history restore
# files = true        # global file send/receive permission
# enabled = false pauses sync both ways; devices stay connected.
# enabled = true

# [sync.directories]
# automatic = false  # copy records references; explicit send prepares the tree

# What the built-in device-to-device sync (builtin.tailcat) sends and accepts.
# Applies to transfers that start after the change. Uncomment the header too.
# [plugins.instances."builtin.tailcat".sync]
# text = true     # text, HTML and other non-image formats
# images = true
# files = true
# Files copied automatically above this size are not sent (and neither is the
# rest of that copy); sending by hand ignores it. 0 = no extra limit.
# auto_file_max_bytes = "100MB"

# builtin.classify adds stable builtin:text/image/files/link tags to new content.
# Link text also receives builtin:text. It never changes content.
# [plugins.instances."builtin.classify"]
# enabled = true

# builtin.text tags copied text that holds an email address ("email") or a
# link ("url"), and offers the text actions in the history window. It only
# adds tags; it never changes what you copied. Needs a restart of unicd.
# [plugins.instances."builtin.text"]
# enabled = true

# Lua plugins you installed under $UNIC_HOME/plugins/lua/<name>/ (a spec.toml
# and its script), run in this order. Saving a script applies it at once;
# changing this list needs a restart of unicd.
# [plugins]
# lua = ["example.lua-urltag"]
#
# What happens to a copy when a plugin fails (error, timeout, crash):
#   "continue"   (default) the failed plugin's tags and settings for this copy
#                are dropped and the copy is synced as usual. A plugin that is
#                meant to keep secrets off other devices then lets them through
#                whenever it fails.
#   "block_sync" the copy stays on this device. No later plugin can lift that.
# Set it for every plugin, or for one plugin, which wins. Needs a restart.
# on_error = "continue"
# [plugins.instances."example.lua-urltag"]
# on_error = "block_sync"
# The plugin's own settings are kept here, not in its directory, so installing
# or upgrading the plugin never replaces them. Needs a restart.
# [plugins.instances."example.lua-urltag".config]
# tag = "link"
`

func validOnError(v string) bool { return v == "" || v == "continue" || v == "block_sync" }

// WriteTemplate creates path with the template unless it already exists.
func WriteTemplate(path string) error {
	f, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		if errors.Is(err, os.ErrExist) {
			return nil
		}
		return err
	}
	_, err = f.WriteString(Template)
	if cerr := f.Close(); err == nil {
		err = cerr
	}
	return err
}

func (c Config) Observability() observability.Config {
	return observability.Config{Logging: c.Logging, Telemetry: c.Telemetry}
}
