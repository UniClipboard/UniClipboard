package luascripts

import (
	"errors"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"unicode"

	"github.com/uniclipboard/unic/plugin"
)

var pluginID = regexp.MustCompile(`^[a-z][a-z0-9]*(\.[a-z][a-z0-9_-]*)+$`)
var hashPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)

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
	if m.SettingsSchema != "" && !localPath(m.SettingsSchema) ||
		m.SettingsSHA256 != "" && (m.SettingsSchema == "" || !hashPattern.MatchString(m.SettingsSHA256)) {
		return errors.New("invalid settings schema declaration")
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

func (m Manifest) validKind(installed bool) bool {
	switch m.Kind {
	case "lua":
		return localPath(m.Entry) && m.Module == "" && m.Package == "" && len(m.Artifacts) == 0 &&
			m.TransportKind == "" && !m.ExternalChannels && !m.RemoteRevocation &&
			!slices.Contains(m.Capabilities, plugin.CapSyncBackend)
	case "go":
		return installed && m.Entry == "" && m.Module != "" &&
			(m.Package == "." || strings.HasPrefix(m.Package, "./") && localPath(strings.TrimPrefix(m.Package, "./")))
	default:
		return false
	}
}

func (m Manifest) PluginSpec() plugin.Spec {
	return plugin.Spec{ID: m.ID, Version: m.Version, APIVersion: m.APIVersion, Capabilities: m.Capabilities,
		SettingsSHA256: m.SettingsSHA256, Actions: m.Actions, TransportKind: m.TransportKind, ExternalChannels: m.ExternalChannels, RemoteRevocation: m.RemoteRevocation}
}
