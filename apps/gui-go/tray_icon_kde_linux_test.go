//go:build linux

package main

import (
	"errors"
	"image/color"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

func kdeTestEnv(t *testing.T) (linuxDesktopEnv, map[string]string) {
	t.Helper()
	root := t.TempDir()
	vars := map[string]string{
		"HOME": root, "XDG_CURRENT_DESKTOP": "KDE",
		"XDG_CONFIG_HOME": filepath.Join(root, "config"),
		"XDG_CONFIG_DIRS": filepath.Join(root, "system-config"),
		"XDG_DATA_HOME":   filepath.Join(root, "data"),
		"XDG_DATA_DIRS":   filepath.Join(root, "system-data"),
	}
	return linuxDesktopEnv{getenv: func(key string) string { return vars[key] }, readFile: readLinuxThemeFile}, vars
}

func kdeWrite(t *testing.T, path, contents string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(contents), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestKDEPanelPalette(t *testing.T) {
	const darkTheme = "plasma/desktoptheme/breeze-dark/colors"
	const fedoraDefaults = "plasma/look-and-feel/org.fedoraproject.fedora.desktop/contents/defaults"
	for _, tt := range []struct {
		name   string
		config map[string]string
		data   map[string]string
		want   color.NRGBA
	}{
		{"Fedora44Plasmarc", map[string]string{
			"plasmarc":   "[Theme]\nname=breeze-dark",
			"kdeglobals": "[General]\nColorScheme=BreezeLight\n[KDE]\nLookAndFeelPackage=org.fedoraproject.fedora.desktop\n[Colors:Window]\nBackgroundNormal=239,240,241",
		}, map[string]string{darkTheme: "[Colors:Window]\nBackgroundNormal=32,35,38"}, linuxLightIcon},
		{"Fedora44LookAndFeel", map[string]string{
			"kdeglobals": "[General]\nColorScheme=BreezeLight\n[KDE]\nLookAndFeelPackage=org.fedoraproject.fedora.desktop\n[Colors:Window]\nBackgroundNormal=239,240,241",
		}, map[string]string{fedoraDefaults: "[plasmarc][Theme]\nname=breeze-dark", darkTheme: "[Colors:Window]\nBackgroundNormal=32,35,38"}, linuxLightIcon},
		{"Breeze", map[string]string{"plasmarc": "[Theme]\nname=default", "kdeglobals": "[Colors:Window]\nBackgroundNormal=239,240,241"}, nil, linuxDarkIcon},
		{"BreezeDark", map[string]string{"plasmarc": "[Theme]\nname=breeze-dark"}, map[string]string{darkTheme: "[Colors:Window]\nBackgroundNormal=32,35,38"}, linuxLightIcon},
		{"ApplicationScheme", map[string]string{"kdeglobals": "[General]\nColorScheme=BreezeDark"}, map[string]string{"color-schemes/BreezeDark.colors": "[Colors:Window]\nBackgroundNormal=32,35,38"}, linuxLightIcon},
		{"LookAndFeelScheme", map[string]string{"kdeglobals": "[KDE]\nLookAndFeelPackage=org.fedoraproject.fedora.desktop"}, map[string]string{fedoraDefaults: "[plasmarc][Theme]\nname=default\n[kdeglobals][General]\nColorScheme=BreezeLight", "color-schemes/BreezeLight.colors": "[Colors:Window]\nBackgroundNormal=239,240,241"}, linuxDarkIcon},
		{"InvalidThemeUsesApplication", map[string]string{"plasmarc": "[Theme]\nname=breeze-dark", "kdeglobals": "[Colors:Window]\nBackgroundNormal=239,240,241"}, map[string]string{darkTheme: "[Colors:Window]\nBackgroundNormal=garbage"}, linuxDarkIcon},
		{"InvalidApplicationUsesScheme", map[string]string{"kdeglobals": "[Colors:Window]\nBackgroundNormal=garbage\n[General]\nColorScheme=BreezeDark"}, map[string]string{"color-schemes/BreezeDark.colors": "[Colors:Window]\nBackgroundNormal=32,35,38"}, linuxLightIcon},
	} {
		t.Run(tt.name, func(t *testing.T) {
			env, vars := kdeTestEnv(t)
			for path, contents := range tt.config {
				kdeWrite(t, filepath.Join(vars["XDG_CONFIG_HOME"], path), contents)
			}
			for path, contents := range tt.data {
				kdeWrite(t, filepath.Join(vars["XDG_DATA_DIRS"], path), contents)
			}
			got := linuxTrayPalette(env, func(string, string) string {
				t.Fatal("valid KDE colour must not query gsettings")
				return ""
			})
			if got != tt.want {
				t.Fatalf("palette = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestKDECascade(t *testing.T) {
	env, vars := kdeTestEnv(t)
	first := vars["XDG_CONFIG_DIRS"]
	second := filepath.Join(vars["HOME"], "second-config")
	vars["XDG_CONFIG_DIRS"] = first + ":" + second
	kdeWrite(t, filepath.Join(second, "plasmarc"), "[Theme]\nname=light")
	kdeWrite(t, filepath.Join(first, "plasmarc"), "[Theme]\nname=dark")
	kdeWrite(t, filepath.Join(vars["XDG_DATA_DIRS"], "plasma/desktoptheme/dark/colors"), "[Colors:Window]\nBackgroundNormal=32,35,38")
	kdeWrite(t, filepath.Join(vars["XDG_DATA_DIRS"], "plasma/desktoptheme/light/colors"), "[Colors:Window]\nBackgroundNormal=239,240,241")
	assertPanel := func(want bool) {
		t.Helper()
		if dark, ok := kdePanelIsDark(env); !ok || dark != want {
			t.Fatalf("panel = (%v, %v), want (%v, true)", dark, ok, want)
		}
	}
	assertPanel(true) // The first system config directory wins.
	kdeWrite(t, filepath.Join(vars["XDG_CONFIG_HOME"], "plasmarc"), "[Theme]\nname=light")
	assertPanel(false)
	kdeWrite(t, filepath.Join(vars["XDG_DATA_HOME"], "plasma/desktoptheme/light/colors"), "[Colors:Window]\nBackgroundNormal=32,35,38")
	assertPanel(true)
	// A higher-priority file without the key does not hide a lower-priority key.
	kdeWrite(t, filepath.Join(vars["XDG_CONFIG_HOME"], "plasmarc"), "[Other]\nname=light")
	assertPanel(true)
	// kdeglobals uses the same per-key cascade, including application colours.
	kdeWrite(t, filepath.Join(vars["XDG_CONFIG_HOME"], "plasmarc"), "[Theme]\nname=default")
	kdeWrite(t, filepath.Join(second, "kdeglobals"), "[Colors:Window]\nBackgroundNormal=239,240,241")
	kdeWrite(t, filepath.Join(first, "kdeglobals"), "[General]\nColorScheme=unused")
	assertPanel(false)
	kdeWrite(t, filepath.Join(vars["XDG_CONFIG_HOME"], "kdeglobals"), "[Colors:Window]\nBackgroundNormal=32,35,38")
	assertPanel(true)
}

func TestLinuxDesktopDetectionAndFallback(t *testing.T) {
	for _, desktop := range []string{"GNOME", "", "ubuntu:GNOME", "NotKDE", "KDE", "X-Generic:kde"} {
		for _, setting := range []string{"'prefer-light'", "'prefer-dark'", "'default'", "", "prefer-light"} {
			t.Run(desktop+"/"+setting, func(t *testing.T) {
				env, vars := kdeTestEnv(t)
				vars["XDG_CURRENT_DESKTOP"] = desktop
				reads, lookups := 0, 0
				env.readFile = func(string) ([]byte, error) { reads++; return nil, os.ErrNotExist }
				got := linuxTrayPalette(env, func(schema, key string) string {
					lookups++
					if schema != "org.gnome.desktop.interface" || key != "color-scheme" {
						t.Fatalf("unexpected gsettings lookup: %s %s", schema, key)
					}
					return setting
				})
				want := linuxLightIcon
				if setting == "'prefer-light'" {
					want = linuxDarkIcon
				}
				if got != want || lookups != 1 {
					t.Fatalf("palette = %v, lookups = %d; want %v, 1", got, lookups, want)
				}
				isKDE := desktop == "KDE" || desktop == "X-Generic:kde"
				if (reads > 0) != isKDE || env.isKDE() != isKDE {
					t.Fatalf("KDE detection = %v, reads = %d", env.isKDE(), reads)
				}
			})
		}
	}
	if linuxLightIcon != (color.NRGBA{0xCD, 0xD6, 0xF4, 255}) || linuxDarkIcon != (color.NRGBA{0x1B, 0x1B, 0x1B, 255}) {
		t.Fatal("Linux palette changed")
	}
}

func TestKDEInvalidFilesFallback(t *testing.T) {
	for _, contents := range []string{"", "not ini\n[broken\nBackgroundNormal=32,35,38", "[Colors:Window]\nBackgroundNormal=256,0,0", strings.Repeat("x", maxLinuxThemeFileSize+1)} {
		for _, setting := range []string{"'prefer-light'", "'prefer-dark'"} {
			env, vars := kdeTestEnv(t)
			kdeWrite(t, filepath.Join(vars["XDG_CONFIG_HOME"], "kdeglobals"), contents)
			calls := 0
			got := linuxTrayPalette(env, func(string, string) string { calls++; return setting })
			want := linuxLightIcon
			if setting == "'prefer-light'" {
				want = linuxDarkIcon
			}
			if got != want || calls != 1 {
				t.Fatalf("invalid KDE data: palette = %v, calls = %d", got, calls)
			}
		}
	}
	env, _ := kdeTestEnv(t)
	env.readFile = func(string) ([]byte, error) { return nil, os.ErrPermission }
	if _, ok := kdePanelIsDark(env); ok {
		t.Fatal("unreadable files yielded a colour")
	}
}

func TestKDEKConfigParser(t *testing.T) {
	config := parseKConfig([]byte(" # comment\n; comment\n\n[Theme]\n name[$e][$i] = breeze-dark \ninvalid\n[plasmarc][Theme]\nname = default\n[kdeglobals][General]\nColorScheme[$i]=BreezeLight\n[Colors:Window]\nBackgroundNormal = 32,35,38,255\n[broken\nname=ignored"))
	for _, tt := range []struct{ group, key, want string }{
		{"Theme", "name", "breeze-dark"},
		{"plasmarc/Theme", "name", "default"},
		{"kdeglobals/General", "ColorScheme", "BreezeLight"},
		{"Colors:Window", "BackgroundNormal", "32,35,38,255"},
	} {
		if got, ok := config.value(tt.group, tt.key); !ok || got != tt.want {
			t.Errorf("%s %s = (%q, %v), want %q", tt.group, tt.key, got, ok, tt.want)
		}
	}
	if _, ok := parseKConfig([]byte(strings.Repeat("x", maxLinuxThemeFileSize+1))).value("Theme", "name"); ok {
		t.Fatal("oversized config parsed")
	}
}

func TestKDEPanelColourThreshold(t *testing.T) {
	for _, tt := range []struct {
		value string
		dark  bool
		ok    bool
	}{
		{"191,191,191", true, true}, {"192,192,192", false, true},
		{"255,208,0", true, true}, {"255,209,0", false, true},
		{"32,35,38,0", true, true}, {" #eff0f1 ", false, true},
		{"#202326", true, true}, {" 239, 240, 241, 255 ", false, true},
		{"-1,0,0", false, false}, {"256,0,0", false, false},
		{"0,0,0,256", false, false}, {"0,0", false, false},
		{"0,0,0,0,0", false, false}, {"#fff", false, false},
		{"1.5,0,0", false, false}, {"garbage", false, false},
	} {
		if dark, ok := kdeColourIsDark(tt.value); dark != tt.dark || ok != tt.ok {
			t.Errorf("colour %q = (%v, %v), want (%v, %v)", tt.value, dark, ok, tt.dark, tt.ok)
		}
	}
}

func TestKDEXDGDefaults(t *testing.T) {
	env := linuxDesktopEnv{getenv: func(key string) string {
		if key == "HOME" {
			return "/test-home"
		}
		return ""
	}}
	if got := env.configDirs(); !reflect.DeepEqual(got, []string{"/test-home/.config", "/etc/xdg"}) {
		t.Fatalf("config dirs = %v", got)
	}
	if got := env.dataDirs(); !reflect.DeepEqual(got, []string{"/test-home/.local/share", "/usr/local/share", "/usr/share"}) {
		t.Fatalf("data dirs = %v", got)
	}
}

func TestLinuxThemeFileReadLimit(t *testing.T) {
	path := filepath.Join(t.TempDir(), "theme")
	kdeWrite(t, path, strings.Repeat("x", maxLinuxThemeFileSize))
	if data, err := readLinuxThemeFile(path); err != nil || len(data) != maxLinuxThemeFileSize {
		t.Fatalf("at limit: bytes = %d, err = %v", len(data), err)
	}
	kdeWrite(t, path, strings.Repeat("x", maxLinuxThemeFileSize+1))
	if _, err := readLinuxThemeFile(path); err == nil {
		t.Fatal("oversized file accepted")
	}
	if _, err := readLinuxThemeFile(filepath.Dir(path)); err == nil {
		t.Fatal("directory accepted")
	}
	if _, err := readLinuxThemeFile(path + "-missing"); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("missing file: %v", err)
	}
}

func TestKDEConfigFingerprintPolling(t *testing.T) {
	env, _ := kdeTestEnv(t)
	calls := 0
	poll := newKDEThemePoll(env, func() { calls++ })
	paths := env.kdeConfigPaths()
	if len(paths) != 4 {
		t.Fatalf("config paths = %v", paths)
	}
	poll()
	if calls != 0 {
		t.Fatal("unchanged missing files triggered repaint")
	}
	for _, path := range paths {
		kdeWrite(t, path, "[Theme]\nname=default")
		poll()
		callsBefore := calls
		poll()
		if callsBefore == 0 || calls != callsBefore {
			t.Fatal("unchanged files triggered repaint")
		}
		before := kdeConfigFingerprint(env)
		kdeWrite(t, path+".new", "[Theme]\nname=breeze-dark")
		if err := os.Rename(path+".new", path); err != nil {
			t.Fatal(err)
		}
		if reflect.DeepEqual(before, kdeConfigFingerprint(env)) {
			t.Fatal("atomic replacement was not detected")
		}
		poll()
		if calls != callsBefore+1 {
			t.Fatal("rewrite did not trigger repaint")
		}
		info, err := os.Stat(path)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.Chtimes(path, info.ModTime(), info.ModTime().Add(time.Second)); err != nil {
			t.Fatal(err)
		}
		poll()
		if calls != callsBefore+2 {
			t.Fatal("mtime-only change did not trigger repaint")
		}
		if err := os.Remove(path); err != nil {
			t.Fatal(err)
		}
		poll()
		if calls != callsBefore+3 {
			t.Fatal("deletion did not trigger repaint")
		}
	}
}
