// Package luascripts finds the Lua plugins installed in the desktop state
// directory and turns the enabled ones into engine plugin specs. A plugin is a
// directory holding spec.toml (plugin-contract, "清单与产物") and its
// entry script. Reading is strict: a malformed manifest or an unsafe entry path
// is an error naming the directory, never the script text.
package luascripts

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/settings"
)

// ManifestFile is the manifest name inside a plugin directory.
const ManifestFile = "spec.toml"

const maxSource = 1 << 20

// Manifest is spec.toml of a Lua plugin.
type Manifest struct {
	ManifestVersion  int                 `toml:"manifest_version"`
	ID               string              `toml:"id"`
	Version          string              `toml:"version"`
	Name             string              `toml:"name"`
	Icon             string              `toml:"icon"`
	Summary          string              `toml:"summary"`
	I18N             map[string]Text     `toml:"i18n"`
	APIVersion       int                 `toml:"api_version"`
	Kind             string              `toml:"kind"`
	Entry            string              `toml:"entry"`
	Capabilities     []string            `toml:"capabilities"`
	Events           []string            `toml:"events"`
	SettingsSchema   string              `toml:"settings_schema"`
	SettingsSHA256   string              `toml:"settings_sha256"`
	Module           string              `toml:"module"`
	Package          string              `toml:"package"`
	TransportKind    string              `toml:"transport_kind"`
	ExternalChannels bool                `toml:"external_channels"`
	RemoteRevocation bool                `toml:"remote_revocation"`
	Actions          []plugin.ActionSpec `toml:"actions"`
	Artifacts        []Artifact          `toml:"artifacts"`
	Defaults         map[string]any      `toml:"config"`
}

// Text is one language's display text for the plugin list and detail page.
// Empty fields fall back to the manifest's default name or summary.
type Text struct {
	Name    string `toml:"name"`
	Summary string `toml:"summary"`
}

type Artifact struct {
	GOOS             string   `toml:"goos"`
	GOARCH           string   `toml:"goarch"`
	Release          string   `toml:"release"`
	Asset            string   `toml:"asset"`
	Executable       string   `toml:"executable"`
	SHA256           string   `toml:"sha256"`
	MinimumOS        string   `toml:"minimum_os"`
	RuntimeLibraries []string `toml:"runtime_libraries"`
}

// Script is one installed plugin: manifest plus entry source.
type Script struct {
	InstanceID      string
	Dir             string
	Manifest        Manifest
	Source          string
	Schema          *settings.Definition
	InitialSettings []byte
	Commit          string
	BinarySHA256    string
	SourceKind      string
	Repository      string
	HasPrevious     bool
	PreviousVersion string
}

// Same reports whether two versions of a script may replace each other
// without a restart: everything but the source must be identical, because
// that is what the engine fixed at startup. on_error and the plugin's config
// are the user's, kept in config.toml, not in the manifest.
func (s Script) Same(o Script) bool {
	a, b := s.Manifest, o.Manifest
	return a.ID == b.ID && a.Version == b.Version && a.APIVersion == b.APIVersion &&
		slices.Equal(sorted(a.Capabilities), sorted(b.Capabilities)) &&
		slices.Equal(a.Events, b.Events) && s.Commit == o.Commit && schemaDigest(s.Schema) == schemaDigest(o.Schema)
}

func sorted(s []string) []string { return slices.Sorted(slices.Values(s)) }

// Read loads one plugin directory.
func Read(dir string) (Script, error) { return ReadManifest(dir, false) }

func ReadManifest(dir string, installed bool) (Script, error) {
	root, err := os.OpenRoot(dir)
	if err != nil {
		return Script{}, err
	}
	defer func() { _ = root.Close() }()
	data, err := readFile(root, ManifestFile, maxSource)
	if err != nil {
		return Script{}, errors.New("plugin manifest unavailable")
	}
	var m Manifest
	md, err := toml.Decode(string(data), &m)
	if err != nil || len(md.Undecoded()) > 0 || (!installed && m.Defaults != nil) {
		return Script{}, errors.New("invalid plugin manifest")
	}
	if err := m.validate(installed); err != nil {
		return Script{}, err
	}
	s := Script{Dir: dir, Manifest: m}
	if m.Kind == "lua" {
		src, err := readFile(root, m.Entry, maxSource)
		if err != nil {
			return Script{}, errors.New("plugin entry unavailable")
		}
		s.Source = string(src)
	}
	if m.SettingsSchema != "" {
		raw, err := readFile(root, m.SettingsSchema, settings.MaxSchemaBytes)
		if err != nil {
			return Script{}, errors.New("plugin schema unavailable")
		}
		if m.SettingsSHA256 != "" && digest(raw) != m.SettingsSHA256 || installed && m.SettingsSHA256 == "" {
			return Script{}, errors.New("plugin schema digest mismatch")
		}
		s.Schema, err = settings.Parse(raw)
		if err != nil {
			return Script{}, err
		}
		defaults := []byte("{}")
		if m.Defaults != nil {
			defaults, err = json.Marshal(m.Defaults)
			if err != nil {
				return Script{}, errors.New("invalid plugin defaults")
			}
		}
		s.InitialSettings, err = s.Schema.NormalizeDefaults(defaults)
		if err != nil {
			return Script{}, err
		}
	}
	return s, nil
}

func readFile(root *os.Root, name string, limit int) ([]byte, error) {
	info, err := root.Lstat(name)
	if err != nil || !info.Mode().IsRegular() || info.Size() > int64(limit) {
		return nil, errors.New("expected a bounded regular file")
	}
	f, err := root.Open(name)
	if err != nil {
		return nil, err
	}
	defer func() { _ = f.Close() }()
	data, err := io.ReadAll(io.LimitReader(f, int64(limit)+1))
	if err != nil || len(data) > limit {
		return nil, errors.New("file read limit exceeded")
	}
	return data, nil
}

func digest(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func schemaDigest(d *settings.Definition) string {
	if d == nil {
		return ""
	}
	return digest(d.JSON())
}

// scanLocal reads every plugin directory under root, keyed by plugin ID. A
// directory that cannot be read is reported in errs and does not hide the
// others; two directories with one ID are both rejected. A missing root is
// an empty result.
func scanLocal(root string) (found map[string]Script, errs map[string]error) {
	found, errs = map[string]Script{}, map[string]error{}
	ents, err := os.ReadDir(root)
	if err != nil {
		if !errors.Is(err, os.ErrNotExist) {
			errs[root] = err
		}
		return
	}
	dup := map[string]bool{}
	for _, e := range ents {
		if !e.IsDir() {
			continue
		}
		dir := filepath.Join(root, e.Name())
		s, err := Read(dir)
		if err != nil {
			errs[dir] = err
			continue
		}
		if _, ok := found[s.Manifest.ID]; ok || dup[s.Manifest.ID] {
			dup[s.Manifest.ID] = true
			delete(found, s.Manifest.ID)
			errs[dir] = fmt.Errorf("%s: plugin id %q is installed twice", e.Name(), s.Manifest.ID)
			continue
		}
		found[s.Manifest.ID] = s
	}
	return
}

// Scan combines local Lua scripts and GitHub installations by ID.
func Scan(root string) (map[string]Script, map[string]error) {
	found, bad := scanLocal(root)
	if root == "" {
		return found, bad
	}
	dir := installedDir(root)
	lock, err := readLock(dir)
	if err != nil {
		bad[dir] = err
		return found, bad
	}
	for id, pin := range lock {
		if _, exists := found[id]; exists {
			delete(found, id)
			bad[id] = errors.New("plugin ID exists in both local and GitHub installations")
			continue
		}
		s, err := readInstalled(dir, id, pin)
		if err != nil {
			bad[id] = err
			continue
		}
		found[id] = s
	}
	return found, bad
}

// Specs returns enabled Lua specs in processing order and all enabled
// manifests for service assembly. Any enabled plugin that is missing or unreadable is
// an error: the daemon must not start with less than the user asked for.
func Specs(root string, enabled []string, settings func(id string) (onError string, cfg map[string]any)) ([]engine.LuaPluginSpec, map[string]Script, error) {
	if len(enabled) == 0 {
		return nil, nil, nil
	}
	found, errs := Scan(root)
	var specs []engine.LuaPluginSpec
	loaded := map[string]Script{}
	for _, id := range enabled {
		s, ok := found[id]
		if !ok {
			for _, err := range errs { // a broken directory may be the one asked for
				return nil, nil, fmt.Errorf("plugin %q is not usable: %w", id, err)
			}
			return nil, nil, fmt.Errorf("plugin %q is not installed", id)
		}
		loaded[id] = s
		m := s.Manifest
		onError, c := settings(id)
		if m.Kind != "lua" {
			continue
		}
		cfg, err := s.RuntimeConfig(root, c)
		if MissingRequiredSettings(err) {
			continue
		}
		if err != nil {
			return nil, nil, err
		}
		specs = append(specs, engine.LuaPluginSpec{ID: m.ID, Version: m.Version, APIVersion: m.APIVersion,
			Capabilities: m.Capabilities, Source: s.Source, Config: cfg, Events: m.Events, OnError: onError})
	}
	return specs, loaded, nil
}

// ForInstance preserves immutable program identity and chooses separate settings.
func (s Script) ForInstance(id string) Script { s.InstanceID = id; return s }
func (s Script) settingsID() string {
	if s.InstanceID != "" {
		return s.InstanceID
	}
	return s.Manifest.ID
}

// ResolveInstance only assigns installed Go sync implementations to runtime aliases.
func ResolveInstance(found map[string]Script, implementation, id string) (Script, error) {
	s, ok := found[implementation]
	if !ok {
		return Script{}, errors.New("plugin implementation is not installed")
	}
	if !ValidID(id) || !ValidID(implementation) {
		return Script{}, errors.New("invalid backend instance identity")
	}
	if id != implementation {
		if _, exists := found[id]; exists {
			return Script{}, errors.New("instance ID conflicts with an installed implementation")
		}
	}
	if id != implementation && (s.Manifest.Kind != "go" || !slices.Contains(s.Manifest.Capabilities, plugin.CapSyncBackend)) {
		return Script{}, errors.New("distinct instances require an external Go sync backend")
	}
	return s.ForInstance(id), nil
}
