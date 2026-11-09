// Package luascripts finds the Lua plugins installed in the desktop state
// directory and turns the enabled ones into engine plugin specs. A plugin is a
// directory holding spec.toml (plugin-contract, "清单与产物") and its
// entry script. Reading is strict: a malformed manifest or an unsafe entry path
// is an error naming the directory, never the script text.
package luascripts

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/plugin"
)

// ManifestFile is the manifest name inside a plugin directory.
const ManifestFile = "spec.toml"

const maxSource = 1 << 20

// Manifest is spec.toml of a Lua plugin.
type Manifest struct {
	ManifestVersion int      `toml:"manifest_version"`
	ID              string   `toml:"id"`
	Version         string   `toml:"version"`
	APIVersion      int      `toml:"api_version"`
	Kind            string   `toml:"kind"`
	Entry           string   `toml:"entry"`
	Capabilities    []string `toml:"capabilities"`
	Events          []string `toml:"events"`
}

// Script is one installed plugin: manifest plus entry source.
type Script struct {
	Dir      string
	Manifest Manifest
	Source   string
}

// Same reports whether two versions of a script may replace each other
// without a restart: everything but the source must be identical, because
// that is what the engine fixed at startup. on_error and the plugin's config
// are the user's, kept in config.toml, not in the manifest.
func (s Script) Same(o Script) bool {
	a, b := s.Manifest, o.Manifest
	return a.ID == b.ID && a.Version == b.Version && a.APIVersion == b.APIVersion &&
		slices.Equal(sorted(a.Capabilities), sorted(b.Capabilities)) &&
		slices.Equal(a.Events, b.Events)
}

func sorted(s []string) []string { return slices.Sorted(slices.Values(s)) }

// Read loads one plugin directory.
func Read(dir string) (Script, error) {
	var m Manifest
	md, err := toml.DecodeFile(filepath.Join(dir, ManifestFile), &m)
	if err != nil {
		return Script{}, fmt.Errorf("%s: manifest: %w", filepath.Base(dir), err)
	}
	if un := md.Undecoded(); len(un) > 0 {
		return Script{}, fmt.Errorf("%s: manifest has unknown keys", filepath.Base(dir))
	}
	switch {
	case m.ManifestVersion != 1:
		return Script{}, fmt.Errorf("%s: manifest_version must be 1", filepath.Base(dir))
	case m.Kind != "lua":
		return Script{}, fmt.Errorf("%s: kind must be \"lua\"", filepath.Base(dir))
	case m.ID == "" || m.Version == "" || m.APIVersion == 0 || len(m.Capabilities) == 0:
		return Script{}, fmt.Errorf("%s: manifest needs id, version, api_version and capabilities", filepath.Base(dir))
	case m.Entry == "" || !filepath.IsLocal(m.Entry):
		return Script{}, fmt.Errorf("%s: entry must be a path inside the plugin directory", filepath.Base(dir))
	}
	path := filepath.Join(dir, m.Entry)
	// The entry must be a regular file itself, not a link out of the directory.
	if fi, err := os.Lstat(path); err != nil || !fi.Mode().IsRegular() || fi.Size() > maxSource {
		return Script{}, fmt.Errorf("%s: entry is not a regular file of at most 1MiB", filepath.Base(dir))
	}
	src, err := os.ReadFile(path)
	if err != nil {
		return Script{}, fmt.Errorf("%s: entry: %w", filepath.Base(dir), err)
	}
	return Script{Dir: dir, Manifest: m, Source: string(src)}, nil
}

// Scan reads every plugin directory under root, keyed by plugin ID. A
// directory that cannot be read is reported in errs and does not hide the
// others; two directories with one ID are both rejected. A missing root is
// an empty result.
func Scan(root string) (found map[string]Script, errs map[string]error) {
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

// Specs returns engine specs for the enabled IDs, in that order, plus the
// scripts they came from. Any enabled plugin that is missing or unreadable is
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
				return nil, nil, fmt.Errorf("plugins.lua: %q is not usable: %w", id, err)
			}
			return nil, nil, fmt.Errorf("plugins.lua: %q is not installed in %s", id, root)
		}
		m := s.Manifest
		onError, c := settings(id)
		cfg := plugin.Config(c)
		specs = append(specs, engine.LuaPluginSpec{ID: m.ID, Version: m.Version, APIVersion: m.APIVersion,
			Capabilities: m.Capabilities, Source: s.Source, Config: cfg, Events: m.Events, OnError: onError})
		loaded[id] = s
	}
	return specs, loaded, nil
}
