//go:build linux

package main

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"os/user"
	"path/filepath"
	"strings"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// loginItem is the OS login-item manager: Wails' `app.Autostart` (an XDG autostart `.desktop` entry), except for an
// AppImage. Wails writes `Exec=` with `os.Executable()`, which inside an AppImage is a path in the image's temporary
// mount (`/tmp/.mount_*/usr/bin/uniclipboard`) that no longer exists after the process exits, and it finds its own
// entry again only by that path. This is a gap of the pinned Wails (pkg/application/autostart.go
// `resolvedExecutable`), so the AppImage case writes the same kind of entry itself, with `Exec=$APPIMAGE`.
func (h *HostService) loginItem() osAutostart {
	if appImage := os.Getenv("APPIMAGE"); appImage != "" {
		name := productName
		if policy, err := currentLoginItemPolicy(); err == nil {
			name = policy.name()
		}
		return appImageAutostart{appImage: appImage, name: name}
	}
	return h.app.Autostart
}

type appImageAutostart struct{ appImage, name string }

// sessionHome is the home directory the desktop session reads its autostart entries from. It is $HOME, except in a
// portable AppImage: the AppImage runtime sets $HOME to the portable `.home` directory, which no login session ever
// reads, so an entry written there would be reported as registered and never start. The passwd database still has
// the user's real home. (An absolute XDG_CONFIG_HOME is unaffected by the runtime and takes precedence in
// xdgAutostartDir, as it does for the session.)
func sessionHome() (string, error) {
	if _, portable := apppaths.PortableHome(); portable {
		account, err := user.Current()
		if err != nil {
			return "", fmt.Errorf("the portable AppImage cannot locate the login session's home directory: %w", err)
		}
		return account.HomeDir, nil
	}
	return os.UserHomeDir()
}

func xdgAutostartDir() (string, error) {
	if config := os.Getenv("XDG_CONFIG_HOME"); config != "" {
		return filepath.Join(config, "autostart"), nil
	}
	home, err := sessionHome()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".config", "autostart"), nil
}

func (a appImageAutostart) entryPath(identifier string) (string, error) {
	if identifier == "" || strings.ContainsAny(identifier, `/\`+"\x00") || strings.HasPrefix(identifier, ".") {
		return "", fmt.Errorf("invalid autostart identifier %q", identifier)
	}
	dir, err := xdgAutostartDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, identifier+".desktop"), nil
}

func (a appImageAutostart) EnableWithOptions(opts application.AutostartOptions) error {
	path, err := a.entryPath(opts.Identifier)
	if err != nil {
		return err
	}
	exec := desktopQuote(a.appImage)
	for _, arg := range opts.Arguments {
		exec += " " + desktopQuote(arg)
	}
	body := fmt.Sprintf("[Desktop Entry]\nType=Application\nName=%s\nExec=%s\nX-GNOME-Autostart-enabled=true\nHidden=false\nNoDisplay=false\nTerminal=false\n",
		strings.NewReplacer("\n", " ", "\r", " ").Replace(opts.Identifier), exec)
	if strings.ContainsAny(a.appImage, "\n\r") {
		return errors.New("the AppImage path contains a line break")
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, []byte(body), 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}

// Disable removes the entry of this item (named like the login item: the product name, plus the profile).
func (a appImageAutostart) Disable() error {
	path, err := a.entryPath(a.name)
	if err != nil {
		return err
	}
	if err := os.Remove(path); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return err
	}
	return nil
}

func (a appImageAutostart) Status() (application.AutostartStatus, error) {
	path, err := a.entryPath(a.name)
	if err != nil {
		return application.AutostartStatus{}, err
	}
	if _, err := os.Stat(path); err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return application.AutostartStatus{}, nil
		}
		return application.AutostartStatus{}, err
	}
	return application.AutostartStatus{Enabled: true, Path: path, Strategy: application.AutostartStrategyXDGAutostart}, nil
}

// desktopQuote escapes one Exec token per the Desktop Entry specification.
func desktopQuote(s string) string {
	var b strings.Builder
	quote := false
	for _, r := range s {
		switch r {
		case '"', '`', '$', '\\':
			b.WriteByte('\\')
			quote = true
		case ' ', '\t':
			quote = true
		}
		b.WriteRune(r)
	}
	if quote {
		return `"` + b.String() + `"`
	}
	return b.String()
}

// sweepLegacyDesktopEntry removes an autostart entry with this login item's own name that launches another
// program. The Tauri shell (auto-launch crate) wrote `<product name>.desktop` for its own binary or AppImage; Wails
// finds its entry only by executable path, so that file would survive a disable and, after an enable that picks a
// different file name, start the app twice. An entry that already launches this installation is left alone. Only
// this instance's own name is read, so a named profile never touches the primary entry.
func (p loginItemPolicy) sweepLegacyDesktopEntry() error {
	dir, err := xdgAutostartDir()
	if err != nil {
		return nil
	}
	path := filepath.Join(dir, p.name()+".desktop")
	data, err := os.ReadFile(path)
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	mine := os.Getenv("APPIMAGE")
	if mine == "" {
		mine = p.Executable
	}
	if sameFile(execOfDesktopEntry(string(data)), mine) {
		return nil
	}
	return os.Remove(path)
}

func sameFile(a, b string) bool {
	if a == "" || b == "" {
		return false
	}
	if ra, err := filepath.EvalSymlinks(a); err == nil {
		a = ra
	}
	if rb, err := filepath.EvalSymlinks(b); err == nil {
		b = rb
	}
	return a == b
}

// execOfDesktopEntry returns the program of the first Exec= line (first token, quoted or not).
func execOfDesktopEntry(contents string) string {
	for _, line := range strings.Split(contents, "\n") {
		line = strings.TrimSpace(line)
		if !strings.HasPrefix(line, "Exec=") {
			continue
		}
		value := strings.TrimSpace(strings.TrimPrefix(line, "Exec="))
		if strings.HasPrefix(value, `"`) {
			end := strings.Index(value[1:], `"`)
			if end < 0 {
				return ""
			}
			return strings.NewReplacer(`\"`, `"`, `\\`, `\`, "\\`", "`", `\$`, "$").Replace(value[1 : 1+end])
		}
		if i := strings.IndexAny(value, " \t"); i >= 0 {
			return value[:i]
		}
		return value
	}
	return ""
}
