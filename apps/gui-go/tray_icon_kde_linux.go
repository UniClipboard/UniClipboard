//go:build linux

package main

import (
	"errors"
	"image/color"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
)

const maxLinuxThemeFileSize = 1 << 20

var (
	linuxLightIcon = color.NRGBA{0xCD, 0xD6, 0xF4, 255}
	linuxDarkIcon  = color.NRGBA{0x1B, 0x1B, 0x1B, 255}
)

type linuxDesktopEnv struct {
	getenv   func(string) string
	readFile func(string) ([]byte, error)
}

func (env linuxDesktopEnv) isKDE() bool {
	for _, desktop := range strings.Split(env.getenv("XDG_CURRENT_DESKTOP"), ":") {
		if strings.EqualFold(desktop, "KDE") {
			return true
		}
	}
	return false
}

// linuxTrayPalette preserves the gsettings rule for non-KDE desktops and for
// KDE installations whose panel colour cannot be resolved.
func linuxTrayPalette(env linuxDesktopEnv, lookup func(string, string) string) color.NRGBA {
	if env.isKDE() {
		if dark, ok := kdePanelIsDark(env); ok {
			if dark {
				return linuxLightIcon
			}
			return linuxDarkIcon
		}
	}
	if lookup("org.gnome.desktop.interface", "color-scheme") == "'prefer-light'" {
		return linuxDarkIcon
	}
	return linuxLightIcon
}

func (env linuxDesktopEnv) xdgDirs(homeKey, homeDefault, dirsKey, dirsDefault string) []string {
	home := env.getenv(homeKey)
	if home == "" && env.getenv("HOME") != "" {
		home = filepath.Join(env.getenv("HOME"), homeDefault)
	}
	dirs := env.getenv(dirsKey)
	if dirs == "" {
		dirs = dirsDefault
	}
	var paths []string
	for _, path := range append([]string{home}, filepath.SplitList(dirs)...) {
		// XDG paths must be absolute; empty entries must not read the working directory.
		if filepath.IsAbs(path) {
			paths = append(paths, path)
		}
	}
	return paths
}

func (env linuxDesktopEnv) configDirs() []string {
	return env.xdgDirs("XDG_CONFIG_HOME", ".config", "XDG_CONFIG_DIRS", "/etc/xdg")
}

func (env linuxDesktopEnv) dataDirs() []string {
	return env.xdgDirs("XDG_DATA_HOME", ".local/share", "XDG_DATA_DIRS", "/usr/local/share:/usr/share")
}

// readLinuxThemeFile bounds allocation even if a file grows while it is read.
func readLinuxThemeFile(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > maxLinuxThemeFileSize {
		return nil, errors.New("invalid theme file size or type")
	}
	data, err := io.ReadAll(io.LimitReader(file, maxLinuxThemeFileSize+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxLinuxThemeFileSize {
		return nil, errors.New("theme file exceeds size limit")
	}
	return data, nil
}

type kconfig map[string]map[string]string

func (config kconfig) value(group, key string) (string, bool) {
	value, ok := config[group][key]
	return value, ok
}

// parseKConfig handles the groups and key flags used by Plasma theme files.
// It deliberately does not expand environment variables in configuration values.
func parseKConfig(data []byte) kconfig {
	config := kconfig{}
	if len(data) > maxLinuxThemeFileSize {
		return config
	}
	group := ""
	for _, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") || strings.HasPrefix(line, ";") {
			continue
		}
		if strings.HasPrefix(line, "[") {
			group = ""
			var groups []string
			for strings.HasPrefix(line, "[") {
				end := strings.IndexByte(line, ']')
				if end < 0 {
					break
				}
				name := strings.TrimSpace(line[1:end])
				if name != "" && !strings.HasPrefix(name, "$") {
					groups = append(groups, name)
				}
				line = strings.TrimSpace(line[end+1:])
			}
			if line == "" {
				group = strings.Join(groups, "/")
			}
			continue
		}
		key, value, ok := strings.Cut(line, "=")
		if !ok || group == "" {
			continue
		}
		if flag := strings.Index(key, "[$"); flag >= 0 {
			key = key[:flag]
		}
		key = strings.TrimSpace(key)
		if key == "" {
			continue
		}
		if config[group] == nil {
			config[group] = map[string]string{}
		}
		config[group][key] = strings.TrimSpace(value)
	}
	return config
}

type kconfigCascade []kconfig

func (configs kconfigCascade) value(group, key string) (string, bool) {
	for _, config := range configs {
		if value, ok := config.value(group, key); ok {
			return value, true
		}
	}
	return "", false
}

func (env linuxDesktopEnv) configCascade(file string) kconfigCascade {
	var configs kconfigCascade
	for _, dir := range env.configDirs() {
		if data, err := env.readFile(filepath.Join(dir, file)); err == nil {
			configs = append(configs, parseKConfig(data))
		}
	}
	return configs
}

// dataConfig selects the first readable file, preserving XDG data precedence.
func (env linuxDesktopEnv) dataConfig(path string) kconfig {
	for _, dir := range env.dataDirs() {
		if data, err := env.readFile(filepath.Join(dir, path)); err == nil {
			return parseKConfig(data)
		}
	}
	return nil
}

func kdeResourceName(name string) bool {
	return name != "" && name != "." && name != ".." && !strings.ContainsAny(name, "/\\")
}

func kdePanelIsDark(env linuxDesktopEnv) (bool, bool) {
	globals := env.configCascade("kdeglobals")
	var defaults kconfig
	if pkg, _ := globals.value("KDE", "LookAndFeelPackage"); kdeResourceName(pkg) {
		defaults = env.dataConfig(filepath.Join("plasma", "look-and-feel", pkg, "contents", "defaults"))
	}
	theme, found := env.configCascade("plasmarc").value("Theme", "name")
	if !found {
		theme, found = defaults.value("plasmarc/Theme", "name")
		if !found {
			theme = "default"
		}
	}
	if kdeResourceName(theme) {
		config := env.dataConfig(filepath.Join("plasma", "desktoptheme", theme, "colors"))
		if value, ok := config.value("Colors:Window", "BackgroundNormal"); ok {
			if dark, valid := kdeColourIsDark(value); valid {
				return dark, true
			}
		}
	}
	if value, ok := globals.value("Colors:Window", "BackgroundNormal"); ok {
		if dark, valid := kdeColourIsDark(value); valid {
			return dark, true
		}
	}
	scheme, found := globals.value("General", "ColorScheme")
	if !found {
		scheme, _ = defaults.value("kdeglobals/General", "ColorScheme")
	}
	if kdeResourceName(scheme) {
		config := env.dataConfig(filepath.Join("color-schemes", scheme+".colors"))
		if value, ok := config.value("Colors:Window", "BackgroundNormal"); ok {
			return kdeColourIsDark(value)
		}
	}
	return false, false
}

func kdeColourIsDark(value string) (bool, bool) {
	value = strings.TrimSpace(value)
	var rgb [3]int
	if len(value) == 7 && value[0] == '#' {
		packed, err := strconv.ParseUint(value[1:], 16, 24)
		if err != nil {
			return false, false
		}
		rgb = [3]int{int(packed >> 16), int(packed >> 8 & 255), int(packed & 255)}
	} else {
		components := strings.Split(value, ",")
		if len(components) != 3 && len(components) != 4 {
			return false, false
		}
		for i, component := range components {
			n, err := strconv.Atoi(strings.TrimSpace(component))
			if err != nil || n < 0 || n > 255 {
				return false, false
			}
			if i < len(rgb) {
				rgb[i] = n
			}
		}
	}
	// Match KDE's qGray threshold, including its integer division.
	return (rgb[0]*11+rgb[1]*16+rgb[2]*5)/32 < 192, true
}

func (env linuxDesktopEnv) kdeConfigPaths() []string {
	var paths []string
	for _, dir := range env.configDirs() {
		for _, file := range []string{"plasmarc", "kdeglobals"} {
			paths = append(paths, filepath.Join(dir, file))
		}
	}
	return paths
}

type kdeFileFingerprint struct {
	path   string
	mtime  int64 // UnixNano, so the comparison does not depend on time.Time's location or monotonic fields
	size   int64
	exists bool
}

func kdeConfigFingerprint(env linuxDesktopEnv) []kdeFileFingerprint {
	var fingerprint []kdeFileFingerprint
	for _, path := range env.kdeConfigPaths() {
		entry := kdeFileFingerprint{path: path}
		if info, err := os.Stat(path); err == nil {
			entry.mtime, entry.size, entry.exists = info.ModTime().UnixNano(), info.Size(), true
		}
		fingerprint = append(fingerprint, entry)
	}
	return fingerprint
}

// newKDEThemePoll snapshots paths, not inodes, because KConfig saves by rename.
func newKDEThemePoll(env linuxDesktopEnv, changed func()) func() {
	previous := kdeConfigFingerprint(env)
	return func() {
		current := kdeConfigFingerprint(env)
		if !slices.Equal(previous, current) {
			previous = current
			changed()
		}
	}
}
