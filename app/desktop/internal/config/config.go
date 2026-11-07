// Package config loads unic's TOML configuration file and keeps it current:
// the file is watched and every valid save takes effect at once. See
// docs/design/decisions/0002-config-file.md.
package config

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/engine"
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
	DeviceName    string  `toml:"device_name" json:"device_name"`
	RendezvousURL string  `toml:"rendezvous_url" json:"rendezvous_url"`
	Inbox         string  `toml:"inbox" json:"inbox"`
	Limits        Limits  `toml:"limits" json:"limits"`
	Sync          Sync    `toml:"sync" json:"sync"`
	History       History `toml:"history" json:"history"`
}

// History bounds the local clipboard history. It is read when the daemon
// starts. Files copied from disk are remembered by path and cost nothing here.
type History struct {
	MaxItems       int  `toml:"max_items" json:"max_items"`
	MaxBytes       Size `toml:"max_bytes" json:"max_bytes"`
	DefaultTTLDays int  `toml:"default_ttl_days" json:"default_ttl_days"` // 0: keep until evicted
}

type Limits struct {
	MaxFileBytes  Size `toml:"max_file_bytes" json:"max_file_bytes"`
	MaxTotalBytes Size `toml:"max_total_bytes" json:"max_total_bytes"`
}

type Sync struct {
	Enabled bool `toml:"enabled" json:"enabled"`
	Text    bool `toml:"text" json:"text"`
	Images  bool `toml:"images" json:"images"`
	Files   bool `toml:"files" json:"files"`
}

// Default returns the configuration used for keys the file leaves out.
func Default() Config {
	return Config{
		Limits:  Limits{MaxFileBytes: HardMaxFile, MaxTotalBytes: HardMaxTotal},
		Sync:    Sync{Enabled: true, Text: true, Images: true, Files: true},
		History: History{MaxItems: 10000, MaxBytes: 4 << 30},
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
		Limits: engine.Limits{MaxFileBytes: int64(c.Limits.MaxFileBytes), MaxTotalBytes: int64(c.Limits.MaxTotalBytes)},
		Sync:   engine.SyncSettings(c.Sync),
	}
}

// Validate checks values; the engine's rules apply to everything the engine
// uses, so the file and the SDK cannot disagree. Messages never include the
// offending values of fields that could carry secrets.
func (c Config) Validate() error {
	if c.Inbox != "" && !filepath.IsAbs(c.Inbox) {
		return errors.New("inbox: must be an absolute path")
	}
	h := c.History
	switch {
	case h.MaxItems <= 0 || h.MaxItems > 1_000_000:
		return errors.New("history.max_items: must be between 1 and 1000000")
	case h.MaxBytes <= 0 || h.MaxBytes > HardMaxHistory:
		return errors.New("history.max_bytes: must be between 1 byte and 8GiB")
	case h.DefaultTTLDays < 0 || h.DefaultTTLDays > 36500:
		return errors.New("history.default_ttl_days: must be between 0 and 36500")
	}
	return c.Settings().Validate()
}

// HistoryOptions is the history section as engine options.
func (c Config) HistoryOptions() engine.HistoryOptions {
	return engine.HistoryOptions{
		MaxItems: c.History.MaxItems, MaxBytes: int64(c.History.MaxBytes),
		DefaultTTL: time.Duration(c.History.DefaultTTLDays) * 24 * time.Hour,
	}
}

// Template is written when no config file exists. Everything is commented
// out, so the file starts as a no-op that documents the options.
const Template = `# unic configuration. Saving this file applies it at once.
# A file with an error is ignored (the previous settings stay) until fixed;
# run ` + "`unic config`" + ` to see the current settings and any error.

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
# max_file_bytes = "2GiB"
# max_total_bytes = "4GiB"

[history]
# Local clipboard history. Needs a restart of unicd. The oldest entries go
# first. Files you copy from disk are remembered by path, not copied, so they
# do not count towards max_bytes (but stop being readable if they are moved).
# max_items = 10000
# max_bytes = "4GiB"
# default_ttl_days = 0   # 0 keeps entries until they are pushed out

[sync]
# enabled = false pauses sync both ways; devices stay connected.
# enabled = true
# text = true     # text, HTML and other non-image formats
# images = true
# files = true
`

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
