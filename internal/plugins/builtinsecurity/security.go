// Package builtinsecurity implements the built-in Sensitive Content Guard
// processor ("builtin.security"). It runs in the Guard stage ahead of standard
// processors and sync backends, detecting secrets, password-manager copies,
// sensitive filenames, and user-defined rules locally with zero network access.
package builtinsecurity

import (
	"context"
	"errors"
	"slices"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/uniclipboard/unic/plugin"
)

const (
	// ID is the stable built-in plugin identifier.
	ID = "builtin.security"

	// ModeBlock prevents sensitive items from being saved to local history or synced.
	ModeBlock = "block"
	// ModeBlockSync allows local history persistence with a tag, but prevents syncing.
	ModeBlockSync = "block_sync"
	// ModeExpire keeps sensitive items locally with a tag and short expiry, without syncing.
	ModeExpire = "expire"

	DefaultExpireSeconds = 300
	MinExpireSeconds     = 10
	MaxExpireSeconds     = 86400
	DefaultTag           = "sensitive"
)

// Settings configures builtin.security. Zero values select safe defaults
// (block mode, all built-in rules enabled, 64 KiB scan prefix, notifications on).
type Settings struct {
	Mode                    string   `toml:"mode,omitempty" json:"mode,omitempty"`
	ExpireSeconds           int      `toml:"expire_seconds,omitempty" json:"expire_seconds,omitempty"`
	Tag                     string   `toml:"tag,omitempty" json:"tag,omitempty"`
	InterceptReceived       *bool    `toml:"intercept_received,omitempty" json:"intercept_received,omitempty"`
	Notify                  *bool    `toml:"notify,omitempty" json:"notify,omitempty"`
	DetectPrivateKeys       *bool    `toml:"detect_private_keys,omitempty" json:"detect_private_keys,omitempty"`
	DetectTokensAndJWT      *bool    `toml:"detect_tokens_and_jwt,omitempty" json:"detect_tokens_and_jwt,omitempty"`
	DetectConnectionStrings *bool    `toml:"detect_connection_strings,omitempty" json:"detect_connection_strings,omitempty"`
	DetectCredentials       *bool    `toml:"detect_credentials,omitempty" json:"detect_credentials,omitempty"`
	DetectPasswordManagers  *bool    `toml:"detect_password_managers,omitempty" json:"detect_password_managers,omitempty"`
	DetectSensitiveFiles    *bool    `toml:"detect_sensitive_files,omitempty" json:"detect_sensitive_files,omitempty"`
	MaxScanBytes            int      `toml:"max_scan_bytes,omitempty" json:"max_scan_bytes,omitempty"`
	BlockedApps             []string `toml:"blocked_apps,omitempty" json:"blocked_apps,omitempty"`
	AllowedApps             []string `toml:"allowed_apps,omitempty" json:"allowed_apps,omitempty"`
	CustomPatterns          []string `toml:"custom_patterns,omitempty" json:"custom_patterns,omitempty"`
	CustomKeywords          []string `toml:"custom_keywords,omitempty" json:"custom_keywords,omitempty"`
}

func boolOrDefault(v *bool, def bool) bool {
	if v == nil {
		return def
	}
	return *v
}

// EffectiveMode returns the configured mode or ModeBlock when omitted.
func (s Settings) EffectiveMode() string {
	if s.Mode == "" {
		return ModeBlock
	}
	return s.Mode
}

// EffectiveExpireSeconds returns the configured expiry duration or the default.
func (s Settings) EffectiveExpireSeconds() int {
	if s.ExpireSeconds == 0 {
		return DefaultExpireSeconds
	}
	return s.ExpireSeconds
}

// EffectiveTag returns the configured tag or DefaultTag when omitted.
func (s Settings) EffectiveTag() string {
	if s.Tag == "" {
		return DefaultTag
	}
	return s.Tag
}

// EffectiveMaxScanBytes returns the configured scan prefix bound or the default.
func (s Settings) EffectiveMaxScanBytes() int {
	if s.MaxScanBytes == 0 {
		return DefaultMaxScanBytes
	}
	return s.MaxScanBytes
}

func (s Settings) InterceptReceivedEnabled() bool  { return boolOrDefault(s.InterceptReceived, true) }
func (s Settings) NotifyEnabled() bool             { return boolOrDefault(s.Notify, true) }
func (s Settings) DetectPrivateKeysEnabled() bool  { return boolOrDefault(s.DetectPrivateKeys, true) }
func (s Settings) DetectTokensAndJWTEnabled() bool { return boolOrDefault(s.DetectTokensAndJWT, true) }
func (s Settings) DetectConnectionStringsEnabled() bool {
	return boolOrDefault(s.DetectConnectionStrings, true)
}
func (s Settings) DetectCredentialsEnabled() bool { return boolOrDefault(s.DetectCredentials, true) }
func (s Settings) DetectPasswordManagersEnabled() bool {
	return boolOrDefault(s.DetectPasswordManagers, true)
}
func (s Settings) DetectSensitiveFilesEnabled() bool {
	return boolOrDefault(s.DetectSensitiveFiles, true)
}

// Validate checks bounds, mode, tag, app entries, and regular expressions.
func (s Settings) Validate() error {
	switch s.EffectiveMode() {
	case ModeBlock, ModeBlockSync, ModeExpire:
	default:
		return errors.New("mode must be block, block_sync or expire")
	}
	if s.ExpireSeconds != 0 && (s.ExpireSeconds < MinExpireSeconds || s.ExpireSeconds > MaxExpireSeconds) {
		return errors.New("expire_seconds must be between 10 and 86400")
	}
	if !validTag(s.EffectiveTag()) {
		return errors.New("invalid sensitive tag")
	}
	if s.MaxScanBytes != 0 && (s.MaxScanBytes < MinMaxScanBytes || s.MaxScanBytes > MaxMaxScanBytes) {
		return errors.New("max_scan_bytes must be between 1024 and 1048576")
	}
	_, err := newDetector(s.detectorOptions())
	return err
}

func (s Settings) detectorOptions() detectorOptions {
	return detectorOptions{
		DetectPrivateKeys:       s.DetectPrivateKeysEnabled(),
		DetectCredentials:       s.DetectCredentialsEnabled(),
		DetectConnectionStrings: s.DetectConnectionStringsEnabled(),
		DetectTokensAndJWT:      s.DetectTokensAndJWTEnabled(),
		DetectPasswordManagers:  s.DetectPasswordManagersEnabled(),
		DetectSensitiveFiles:    s.DetectSensitiveFilesEnabled(),
		MaxScanBytes:            s.EffectiveMaxScanBytes(),
		BlockedApps:             s.BlockedApps,
		AllowedApps:             s.AllowedApps,
		CustomPatterns:          s.CustomPatterns,
		CustomKeywords:          s.CustomKeywords,
	}
}

func canonicalList(in []string) []string {
	if len(in) == 0 {
		return nil
	}
	out := make([]string, 0, len(in))
	for _, v := range in {
		v = strings.TrimSpace(v)
		if v != "" {
			out = append(out, v)
		}
	}
	if len(out) == 0 {
		return nil
	}
	return out
}

// Normalized returns a canonical Settings value with all defaults materialized
// for deep-equality restart checks.
func (s Settings) Normalized() Settings {
	ir := s.InterceptReceivedEnabled()
	notify := s.NotifyEnabled()
	pk := s.DetectPrivateKeysEnabled()
	tok := s.DetectTokensAndJWTEnabled()
	cs := s.DetectConnectionStringsEnabled()
	cred := s.DetectCredentialsEnabled()
	pm := s.DetectPasswordManagersEnabled()
	sf := s.DetectSensitiveFilesEnabled()
	return Settings{
		Mode:                    s.EffectiveMode(),
		ExpireSeconds:           s.EffectiveExpireSeconds(),
		Tag:                     s.EffectiveTag(),
		InterceptReceived:       &ir,
		Notify:                  &notify,
		DetectPrivateKeys:       &pk,
		DetectTokensAndJWT:      &tok,
		DetectConnectionStrings: &cs,
		DetectCredentials:       &cred,
		DetectPasswordManagers:  &pm,
		DetectSensitiveFiles:    &sf,
		MaxScanBytes:            s.EffectiveMaxScanBytes(),
		BlockedApps:             canonicalList(s.BlockedApps),
		AllowedApps:             canonicalList(s.AllowedApps),
		CustomPatterns:          slices.Clone(s.CustomPatterns),
		CustomKeywords:          canonicalList(s.CustomKeywords),
	}
}

// Plugin is the in-process Sensitive Content Guard processor.
type Plugin struct {
	host     plugin.Host
	settings Settings
	detector *Detector
}

// New validates settings and constructs the built-in security guard plugin.
func New(s Settings) (*Plugin, error) {
	if err := s.Validate(); err != nil {
		return nil, err
	}
	norm := s.Normalized()
	det, err := newDetector(norm.detectorOptions())
	if err != nil {
		return nil, err
	}
	return &Plugin{
		settings: norm,
		detector: det,
	}, nil
}

// Spec declares builtin.security as a Guard-stage processor.
func (*Plugin) Spec() plugin.Spec {
	return plugin.Spec{
		ID:             ID,
		Version:        "1.0.0",
		APIVersion:     1,
		Capabilities:   []string{plugin.CapProcessor},
		ProcessorStage: plugin.ProcessorStageGuard,
	}
}

func (p *Plugin) Init(_ context.Context, host plugin.Host, cfg plugin.Config) error {
	if len(cfg) != 0 {
		return errors.New("builtin.security takes no raw config map")
	}
	p.host = host
	return nil
}

func (*Plugin) Start(context.Context) error { return nil }

func (p *Plugin) Close(context.Context) error {
	p.host = nil
	return nil
}

// Process inspects each captured or received clipboard item and applies the
// configured interception policy when sensitive content is detected.
func (p *Plugin) Process(ctx context.Context, c plugin.ProcessContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if p.detector == nil {
		return nil
	}
	if c.Event() == plugin.EventReceived && !p.settings.InterceptReceivedEnabled() {
		return nil
	}

	match, sensitive, err := p.detector.Evaluate(ctx, c.Item())
	if err != nil {
		return err
	}
	if !sensitive {
		return nil
	}

	switch p.settings.EffectiveMode() {
	case ModeBlock:
		if err := c.SetPersist(false); err != nil {
			return err
		}
		if err := c.SetSync(false); err != nil {
			return err
		}
	case ModeBlockSync:
		if err := c.SetSync(false); err != nil {
			return err
		}
		if err := c.AddTag(p.settings.EffectiveTag()); err != nil {
			return err
		}
	case ModeExpire:
		if err := c.SetSync(false); err != nil {
			return err
		}
		if err := c.AddTag(p.settings.EffectiveTag()); err != nil {
			return err
		}
		if err := c.SetExpire(time.Duration(p.settings.EffectiveExpireSeconds()) * time.Second); err != nil {
			return err
		}
	default:
		return plugin.ErrInvalidConfig
	}

	if p.settings.NotifyEnabled() && p.host != nil {
		p.host.Notify(ctx, notificationText(p.settings.EffectiveMode(), match.Category))
	}
	return nil
}

func validTag(tag string) bool {
	trimmed := strings.TrimSpace(tag)
	return trimmed != "" && trimmed == tag && utf8.ValidString(tag) && len(tag) <= 64 && !strings.ContainsFunc(tag, unicode.IsControl)
}

// notificationText formats a bounded, content-free user notification naming
// only the generic interception mode and rule category.
func notificationText(mode string, category Category) string {
	label := categoryLabel(category)
	switch mode {
	case ModeBlock:
		return "Blocked sensitive content (" + label + "): not saved to history or synced."
	case ModeBlockSync:
		return "Protected sensitive content (" + label + "): sync disabled."
	case ModeExpire:
		return "Protected sensitive content (" + label + "): local-only with auto-expiry."
	default:
		return "Protected sensitive content (" + label + ")."
	}
}

func categoryLabel(category Category) string {
	switch category {
	case CategoryPasswordManager:
		return "password manager"
	case CategoryBlockedApp:
		return "blocked app"
	case CategoryPrivateKey:
		return "private key"
	case CategoryToken:
		return "API token"
	case CategoryConnectionString:
		return "connection string"
	case CategoryCredential:
		return "credential"
	case CategorySensitiveFile:
		return "sensitive file"
	case CategoryCustomPattern:
		return "custom rule"
	case CategoryCustomKeyword:
		return "custom keyword"
	default:
		return "sensitive data"
	}
}
