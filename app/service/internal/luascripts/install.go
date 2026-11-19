package luascripts

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"runtime"

	"github.com/rogpeppe/go-internal/lockedfile"
	"github.com/uniclipboard/unic/internal/luaplugin"
)

type InstallOptions struct {
	Repository string
	Ref        string
	Path       string
	Build      bool
	Replace    bool
}

type installedPlugin struct {
	Repository string           `json:"repository"`
	Commit     string           `json:"commit"`
	Path       string           `json:"path,omitempty"`
	Version    string           `json:"version"`
	API        int              `json:"api_version"`
	Source     string           `json:"source"`
	Manifest   string           `json:"manifest_sha256"`
	Digest     string           `json:"sha256"`
	GOOS       string           `json:"goos,omitempty"`
	GOARCH     string           `json:"goarch,omitempty"`
	Release    string           `json:"release,omitempty"`
	Asset      string           `json:"asset,omitempty"`
	Toolchain  string           `json:"toolchain,omitempty"`
	ModuleSum  string           `json:"module_sum_sha256,omitempty"`
	Previous   *installedPlugin `json:"previous,omitempty"`
}

var commitPattern = regexp.MustCompile(`^[a-f0-9]{40}$`)

func installedDir(root string) string { return filepath.Join(filepath.Dir(root), "installed") }

func readLock(dir string) (map[string]installedPlugin, error) {
	r, err := os.OpenRoot(dir)
	if errors.Is(err, os.ErrNotExist) {
		return map[string]installedPlugin{}, nil
	}
	if err != nil {
		return nil, err
	}
	defer r.Close()
	raw, err := readFile(r, "installed.lock", 1<<20)
	if _, statErr := r.Lstat("installed.lock"); errors.Is(statErr, os.ErrNotExist) {
		return map[string]installedPlugin{}, nil
	}
	var lock map[string]installedPlugin
	if err != nil || json.Unmarshal(raw, &lock) != nil || lock == nil {
		return nil, errors.New("invalid plugin installation lock")
	}
	for id, pin := range lock {
		if !ValidID(id) || !validInstallation(pin) ||
			pin.Previous != nil && (!validInstallation(*pin.Previous) || pin.Previous.Previous != nil || pin.Previous.Repository != pin.Repository || pin.Previous.Path != pin.Path) {
			return nil, errors.New("invalid plugin installation lock")
		}
	}
	return lock, nil
}

func validInstallation(pin installedPlugin) bool {
	return commitPattern.MatchString(pin.Commit) && hashPattern.MatchString(pin.Manifest) && hashPattern.MatchString(pin.Digest)
}

func writeLock(dir string, lock map[string]installedPlugin) error {
	raw, err := json.MarshalIndent(lock, "", "  ")
	if err != nil {
		return err
	}
	if len(raw) > 1<<20 {
		return errors.New("plugin installation lock is full")
	}
	return atomicWrite(dir, "installed.lock", raw)
}

func editLock(root string) (string, map[string]installedPlugin, func(), error) {
	dir := installedDir(root)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return "", nil, nil, err
	}
	unlock, err := lockedfile.MutexAt(filepath.Join(dir, ".lock")).Lock()
	if err != nil {
		return "", nil, nil, err
	}
	lock, err := readLock(dir)
	if err != nil {
		unlock()
		return "", nil, nil, err
	}
	return dir, lock, unlock, nil
}

func Install(ctx context.Context, root string, o InstallOptions) (string, error) {
	repo, err := openGitHub(o.Repository)
	if err != nil {
		return "", err
	}
	if o.Path != "" && !localPath(o.Path) {
		return "", errors.New("invalid plugin repository path")
	}
	commit, err := repo.commit(ctx, o.Ref)
	if err != nil {
		return "", err
	}
	dir := installedDir(root)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return "", err
	}
	stage, err := os.MkdirTemp(dir, ".stage-*")
	if err != nil {
		return "", err
	}
	defer os.RemoveAll(stage)
	source := filepath.Join(stage, "source")
	if err := repo.source(ctx, commit, source); err != nil {
		return "", err
	}
	work := filepath.Join(source, o.Path)
	script, err := ReadManifest(work, true)
	if err != nil {
		return "", err
	}
	id := script.Manifest.ID
	pin := installedPlugin{Repository: repo.location, Commit: commit, Path: o.Path, Version: script.Manifest.Version, API: script.Manifest.APIVersion}
	raw, err := os.ReadFile(filepath.Join(work, ManifestFile))
	if err != nil {
		return "", err
	}
	pin.Manifest = digest(raw)
	dir, lock, unlock, err := editLock(root)
	if err != nil {
		return "", err
	}
	defer unlock()
	local, _ := scanLocal(root)
	if _, exists := local[id]; exists {
		return "", errors.New("plugin ID already exists in the local Lua directory")
	}
	if old, ok := lock[id]; ok {
		if old.Repository != pin.Repository || old.Path != pin.Path {
			return "", errors.New("plugin ID belongs to another repository or path")
		}
		if old.Commit == commit {
			_, err := readInstalled(dir, id, old)
			return id, err
		}
		if !o.Replace {
			return "", errors.New("plugin is installed at a different commit; use --replace")
		}
		old.Previous = nil
		pin.Previous = &old
	}
	bundle := filepath.Join(stage, "bundle")
	if err := os.Mkdir(bundle, 0700); err != nil {
		return "", err
	}
	names := []string{ManifestFile, script.Manifest.SettingsSchema}
	switch script.Manifest.Kind {
	case "lua":
		m := script.Manifest
		p, err := luaplugin.New(luaplugin.Spec{ID: id, Version: m.Version, APIVersion: m.APIVersion, Capabilities: m.Capabilities, Source: script.Source})
		if err != nil {
			return "", errors.New("Lua plugin failed validation")
		}
		_ = p.Close(ctx)
		names = append(names, m.Entry)
		pin.Source, pin.Digest = "lua", digest([]byte(script.Source))
	case "go":
		if err := repo.prepareGo(ctx, stage, work, bundle, script.Manifest, o.Build, &pin); err != nil {
			return "", err
		}
	}
	for _, name := range names {
		if name != "" {
			if err := copyPluginFile(filepath.Join(work, name), filepath.Join(bundle, name), 0600); err != nil {
				return "", err
			}
		}
	}
	target := filepath.Join(dir, id, commit)
	if err := os.MkdirAll(filepath.Dir(target), 0700); err != nil {
		return "", err
	}
	if _, err := os.Stat(target); err == nil {
		if _, err := readInstalled(dir, id, pin); err != nil {
			return "", errors.New("existing plugin version has different contents")
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", err
	} else if err := os.Rename(bundle, target); err != nil {
		return "", err
	}
	lock[id] = pin
	if err := writeLock(dir, lock); err != nil {
		return "", err
	}
	return id, nil
}

func Uninstall(root, id string, disable func() error) error {
	dir, lock, unlock, err := editLock(root)
	if err != nil {
		return err
	}
	defer unlock()
	if _, ok := lock[id]; !ok {
		return errors.New("plugin is not installed from GitHub")
	}
	if err := disable(); err != nil {
		return err
	}
	delete(lock, id)
	// A running process may reopen its pinned executable during RPC recovery.
	// Keep the immutable version directory until it can no longer be referenced.
	return writeLock(dir, lock)
}

// Rollback switches the desired installation to its previous immutable bundle.
func Rollback(root, id string) error {
	dir, lock, unlock, err := editLock(root)
	if err != nil {
		return err
	}
	defer unlock()
	pin := lock[id]
	if pin.Previous == nil {
		return errors.New("plugin has no previous installation")
	}
	previous := *pin.Previous
	if _, err := readInstalled(dir, id, previous); err != nil {
		return err
	}
	pin.Previous = nil
	previous.Previous = &pin
	lock[id] = previous
	return writeLock(dir, lock)
}

func readInstalled(dir, id string, pin installedPlugin) (Script, error) {
	version := filepath.Join(dir, id, pin.Commit)
	s, err := ReadManifest(version, true)
	if err != nil {
		return Script{}, err
	}
	raw, err := os.ReadFile(filepath.Join(version, ManifestFile))
	if err != nil || digest(raw) != pin.Manifest || s.Manifest.ID != id || s.Manifest.Version != pin.Version || s.Manifest.APIVersion != pin.API {
		return Script{}, errors.New("installed plugin manifest mismatch")
	}
	if s.Manifest.Kind == "lua" {
		if digest([]byte(s.Source)) != pin.Digest {
			return Script{}, errors.New("installed Lua source digest mismatch")
		}
	} else {
		if pin.GOOS != runtime.GOOS || pin.GOARCH != runtime.GOARCH {
			return Script{}, fmt.Errorf("plugin %s was installed for another platform", id)
		}
		if fi, err := os.Lstat(s.Executable()); err != nil || !fi.Mode().IsRegular() || fi.Size() > archiveLimit {
			return Script{}, errors.New("installed Go plugin executable unavailable")
		}
		s.BinarySHA256 = pin.Digest
	}
	s.Commit, s.SourceKind, s.Repository = pin.Commit, pin.Source, pin.Repository
	return s, nil
}

func (s Script) Executable() string {
	name := "plugin"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	return filepath.Join(s.Dir, "bin", name)
}

// Prune removes version directories which neither the installation lock nor
// the active service can reference. Call only after Engine owns this profile.
func Prune(root string, running map[string]Script) error {
	if _, err := os.Stat(installedDir(root)); errors.Is(err, os.ErrNotExist) {
		return nil
	}
	dir, lock, unlock, err := editLock(root)
	if err != nil {
		return err
	}
	defer unlock()
	entries, err := os.ReadDir(dir)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		id := entry.Name()
		if !entry.IsDir() || !ValidID(id) {
			continue
		}
		versions, err := os.ReadDir(filepath.Join(dir, id))
		if err != nil {
			return err
		}
		pin := lock[id]
		for _, version := range versions {
			commit := version.Name()
			keep := commit == pin.Commit || commit == running[id].Commit || pin.Previous != nil && commit == pin.Previous.Commit
			if keep || !version.IsDir() || !commitPattern.MatchString(commit) {
				continue
			}
			if err := os.RemoveAll(filepath.Join(dir, id, commit)); err != nil {
				return err
			}
		}
	}
	return nil
}
