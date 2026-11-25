package luascripts

import (
	"errors"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"unicode"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/settings"
)

var pluginID = regexp.MustCompile(`^[a-z][a-z0-9]*(\.[a-z][a-z0-9_-]*)+$`)
var hashPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)
var iconPattern = regexp.MustCompile(`^[a-z][a-z0-9-]{0,31}$`)

func validIcon(icon string) bool {
	return icon == "" || iconPattern.MatchString(icon)
}

func ValidID(id string) bool {
	return len(id) <= 128 && pluginID.MatchString(id) && !strings.HasPrefix(id, "builtin.")
}

func localPath(s string) bool {
	return filepath.IsLocal(s) && !strings.ContainsAny(s, "\\\x00:") && filepath.ToSlash(filepath.Clean(s)) == s && s != "."
}

func (m Manifest) validate(installed bool) error {
	if !ValidID(m.ID) || m.ManifestVersion != 1 || m.APIVersion != 1 || len(m.Version) == 0 || len(m.Version) > 64 || strings.ContainsFunc(m.Version, unicode.IsControl) ||
		len(m.Capabilities) == 0 || !m.validKind(installed) {
		return errors.New("invalid plugin identity or kind")
	}
	if err := m.validateDisplay(); err != nil {
		return err
	}
	if !validIcon(m.Icon) {
		return errors.New("invalid plugin icon")
	}
	if m.SettingsSchema != "" && !localPath(m.SettingsSchema) ||
		m.SettingsSHA256 != "" && (m.SettingsSchema == "" || !hashPattern.MatchString(m.SettingsSHA256)) {
		return errors.New("invalid settings schema declaration")
	}
	if m.ImageFilesAsImages && (m.Kind != "go" || !m.ExternalChannels || !slices.Contains(m.Capabilities, plugin.CapSyncBackend)) {
		return errors.New("image file projection requires an external Go sync backend")
	}
	platforms := map[string]bool{}
	for _, a := range m.Artifacts {
		key := a.GOOS + "/" + a.GOARCH
		if a.GOOS == "" || a.GOARCH == "" || platforms[key] || a.Release == "" || a.Release == "." || a.Release == ".." || len(a.Release) > 256 || strings.ContainsAny(a.Release, "\x00\r\n") ||
			!localPath(a.Asset) || strings.Contains(a.Asset, "/") || !localPath(a.Executable) || !hashPattern.MatchString(a.SHA256) {
			return errors.New("invalid or duplicate plugin artifact")
		}
		platforms[key] = true
	}
	return nil
}

const (
	maxNameRunes    = 64
	maxSummaryRunes = 200
)

// validateDisplay bounds the optional display name, summary and per-language
// overrides. They are plain text and never affect plugin identity.
func (m Manifest) validateDisplay() error {
	bad := !settings.ValidText(m.Name, maxNameRunes) || !settings.ValidText(m.Summary, maxSummaryRunes) || len(m.I18N) > settings.MaxLanguages
	for tag, text := range m.I18N {
		bad = bad || !settings.ValidLanguage(tag) || !settings.ValidText(text.Name, maxNameRunes) || !settings.ValidText(text.Summary, maxSummaryRunes)
	}
	if bad {
		return errors.New("invalid plugin display text")
	}
	return nil
}

func (m Manifest) validKind(installed bool) bool {
	switch m.Kind {
	case "lua":
		return localPath(m.Entry) && m.Module == "" && m.Package == "" && len(m.Artifacts) == 0 &&
			m.TransportKind == "" && !m.ExternalChannels && !m.RemoteRevocation && m.Enricher == nil &&
			!slices.Contains(m.Capabilities, plugin.CapSyncBackend) && !slices.Contains(m.Capabilities, plugin.CapEnricher)
	case "go":
		return installed && m.Entry == "" && m.Module != "" &&
			(m.Package == "." || strings.HasPrefix(m.Package, "./") && localPath(strings.TrimPrefix(m.Package, "./")))
	default:
		return false
	}
}

func (m Manifest) PluginSpec() plugin.Spec {
	return plugin.Spec{ID: m.ID, Version: m.Version, APIVersion: m.APIVersion, Capabilities: m.Capabilities,
		SettingsSHA256: m.SettingsSHA256, Actions: m.Actions, Enricher: m.Enricher, TransportKind: m.TransportKind, ExternalChannels: m.ExternalChannels, RemoteRevocation: m.RemoteRevocation}
}
