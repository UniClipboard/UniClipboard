// Package paths locates unic's files. Everything lives in one directory so a
// second instance (tests, a different profile) can be run with --profile or UNIC_HOME.
package paths

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"slices"
	"strings"
	"sync"

	"github.com/uniclipboard/unic/internal/releaseinfo"
)

const DefaultProfileName = "default"

var validProfileRe = regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9._-]{0,31}$`)

var (
	profileMu       sync.RWMutex
	explicitProfile string // "", "default", or validated named profile
)

// ProfileInfo describes one local profile directory without mutating disk state.
type ProfileInfo struct {
	Name    string `json:"name"`
	Dir     string `json:"directory"`
	Socket  string `json:"socket"`
	Default bool   `json:"default"`
	Active  bool   `json:"active"`
	Exists  bool   `json:"exists"`
}

func baseDirName() string {
	if releaseinfo.Version == "dev" {
		return "unic-development"
	}
	return "unic"
}

// ValidateProfileName checks that name is a safe named profile identifier.
// Empty string and "default" are accepted as the default profile.
func ValidateProfileName(name string) error {
	if name == "" || name == DefaultProfileName {
		return nil
	}
	if !validProfileRe.MatchString(name) || strings.Contains(name, "..") {
		return fmt.Errorf("invalid profile name %q (use 1-32 letters, digits, '.', '_' or '-')", name)
	}
	if baseDirName() == "unic" && (name == "development" || strings.HasPrefix(name, "development-")) {
		return fmt.Errorf("profile name %q is reserved for development builds", name)
	}
	return nil
}

// SetProfile selects a process-wide CLI/daemon profile override (from --profile).
// Passing "" clears the explicit override.
func SetProfile(name string) error {
	if name == "" {
		profileMu.Lock()
		explicitProfile = ""
		profileMu.Unlock()
		return nil
	}
	if err := ValidateProfileName(name); err != nil {
		return err
	}
	profileMu.Lock()
	explicitProfile = name
	profileMu.Unlock()
	return nil
}

// ExplicitProfile reports the active --profile override, if any.
func ExplicitProfile() string {
	profileMu.RLock()
	defer profileMu.RUnlock()
	return explicitProfile
}

// ActiveProfile returns the display name of the active profile and whether it
// came from an explicit UNIC_HOME path override rather than a named profile.
func ActiveProfile() (name string, customHome bool) {
	if p := ExplicitProfile(); p != "" {
		return p, false
	}
	if os.Getenv("UNIC_HOME") != "" {
		return "custom", true
	}
	if p := strings.TrimSpace(os.Getenv("UNIC_PROFILE")); p != "" {
		return p, false
	}
	return DefaultProfileName, false
}

// V1DataDir locates the original default profile without creating directories.
func V1DataDir() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	var base string
	switch runtime.GOOS {
	case "darwin":
		base = filepath.Join(home, "Library", "Application Support")
	case "windows":
		base = os.Getenv("LOCALAPPDATA")
	default:
		base = os.Getenv("XDG_DATA_HOME")
		if base == "" {
			base = filepath.Join(home, ".local", "share")
		}
	}
	if base == "" {
		return "", errors.New("cannot locate v1 data directory")
	}
	return filepath.Join(base, "app.uniclipboard.desktop"), nil
}

func profileDir(name string) (string, error) {
	if err := ValidateProfileName(name); err != nil {
		return "", err
	}
	base, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	dirName := baseDirName()
	if name != "" && name != DefaultProfileName {
		dirName += "-" + name
	}
	return filepath.Join(base, dirName), nil
}

// ResolveDir selects the profile without creating it. Offline migration and
// read-only inspection must not mutate target directories before validation.
// Precedence: explicit --profile > UNIC_HOME > UNIC_PROFILE > portable.dat > default.
func ResolveDir() (string, error) {
	if p := ExplicitProfile(); p != "" {
		return profileDir(p)
	}
	if d := os.Getenv("UNIC_HOME"); d != "" {
		return filepath.Abs(d)
	}
	if p := strings.TrimSpace(os.Getenv("UNIC_PROFILE")); p != "" {
		return profileDir(p)
	}
	executable, err := os.Executable()
	if err != nil {
		return "", err
	}
	executable, err = filepath.EvalSymlinks(executable)
	if err != nil {
		return "", err
	}
	portable := filepath.Join(filepath.Dir(executable), "portable.dat")
	if info, err := os.Lstat(portable); err == nil {
		if !info.Mode().IsRegular() {
			return "", errors.New("portable.dat must be a regular file")
		}
		return filepath.Join(filepath.Dir(executable), "data"), nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	return profileDir(DefaultProfileName)
}

// CanonicalDir resolves the directory path without creating it; if the directory
// already exists it resolves symlinks so it matches what unicd reports.
func CanonicalDir() (string, error) {
	dir, err := ResolveDir()
	if err != nil {
		return "", err
	}
	if resolved, err := filepath.EvalSymlinks(dir); err == nil {
		return resolved, nil
	}
	return dir, nil
}

// Dir is the state directory: database, socket, received files.
func Dir() (string, error) {
	dir, err := ResolveDir()
	if err != nil {
		return "", err
	}
	if err = os.MkdirAll(dir, 0700); err != nil {
		return "", err
	}
	return filepath.EvalSymlinks(dir)
}

func sub(name string) (string, error) {
	d, err := Dir()
	if err != nil {
		return "", err
	}
	return filepath.Join(d, name), nil
}

// ResolveSocket returns the daemon control socket path without creating the
// state directory.
func ResolveSocket() (string, error) {
	if ExplicitProfile() == "" {
		if s := os.Getenv("UNIC_SOCK"); s != "" {
			return s, nil
		}
	}
	d, err := CanonicalDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(d, "unic.sock"), nil
}

// ResolveConfig returns the TOML config file path without creating the state directory.
func ResolveConfig() (string, error) {
	d, err := CanonicalDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(d, "config.toml"), nil
}

// Socket is the daemon's control socket, the way /var/run/docker.sock is
// docker's. AF_UNIX works on Windows 10+ too, so no named pipes are needed.
// Unix socket paths are short (104 bytes on macOS), so the default sits in the
// state directory and UNIC_HOME users should keep it short.
func Socket() (string, error) {
	if ExplicitProfile() == "" {
		if s := os.Getenv("UNIC_SOCK"); s != "" {
			return s, nil
		}
	}
	return sub("unic.sock")
}

func Database() (string, error) { return sub("unic.db") }
func Inbox() (string, error)    { return sub("inbox") }

// LuaDir is where installed Lua plugins live, one directory each.
func LuaDir() (string, error) { return sub(filepath.Join("plugins", "lua")) }

// Config is the TOML configuration file.
func Config() (string, error) { return sub("config.toml") }

// ListProfiles discovers the default profile and named profiles in the user
// config directory without creating or mutating any files.
func ListProfiles() ([]ProfileInfo, error) {
	activeDir, err := CanonicalDir()
	if err != nil {
		return nil, err
	}
	activeSock, err := ResolveSocket()
	if err != nil {
		return nil, err
	}
	base, err := os.UserConfigDir()
	if err != nil {
		return nil, err
	}
	rootName := baseDirName()
	prefix := rootName + "-"

	defaultDir := filepath.Join(base, rootName)
	if resolved, err := filepath.EvalSymlinks(defaultDir); err == nil {
		defaultDir = resolved
	}
	defaultExists := false
	if st, err := os.Stat(defaultDir); err == nil && st.IsDir() {
		defaultExists = true
	}

	profiles := []ProfileInfo{{
		Name:    DefaultProfileName,
		Dir:     defaultDir,
		Socket:  filepath.Join(defaultDir, "unic.sock"),
		Default: true,
		Active:  defaultDir == activeDir,
		Exists:  defaultExists,
	}}
	if profiles[0].Active {
		profiles[0].Socket = activeSock
	}

	entries, err := os.ReadDir(base)
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	var named []ProfileInfo
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		name := entry.Name()
		if !strings.HasPrefix(name, prefix) {
			continue
		}
		profileName := strings.TrimPrefix(name, prefix)
		if ValidateProfileName(profileName) != nil || profileName == "" || profileName == DefaultProfileName {
			continue
		}
		dir := filepath.Join(base, name)
		if resolved, err := filepath.EvalSymlinks(dir); err == nil {
			dir = resolved
		}
		sock := filepath.Join(dir, "unic.sock")
		active := dir == activeDir
		if active {
			sock = activeSock
		}
		named = append(named, ProfileInfo{
			Name:   profileName,
			Dir:    dir,
			Socket: sock,
			Active: active,
			Exists: true,
		})
	}
	slices.SortFunc(named, func(a, b ProfileInfo) int { return strings.Compare(a.Name, b.Name) })
	profiles = append(profiles, named...)

	hasActive := false
	for _, p := range profiles {
		if p.Active {
			hasActive = true
			break
		}
	}
	if !hasActive {
		activeName, _ := ActiveProfile()
		exists := false
		if st, err := os.Stat(activeDir); err == nil && st.IsDir() {
			exists = true
		}
		profiles = append(profiles, ProfileInfo{
			Name:   activeName,
			Dir:    activeDir,
			Socket: activeSock,
			Active: true,
			Exists: exists,
		})
	}
	return profiles, nil
}
