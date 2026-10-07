package autostart

import (
	"errors"
	"fmt"
	"html"
	"os"
	"path/filepath"
)

func agentFile(name string) (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, "Library", "LaunchAgents", name+".plist"), nil
}

// Enable writes the LaunchAgent, always pointing at the current executable so a stale entry left by an
// older install, a dev build or a moved binary heals itself.
func (r Registration) Enable() error {
	if !filepath.IsAbs(r.Executable) {
		return fmt.Errorf("app path is not absolute: %s", r.Executable)
	}
	if _, err := os.Stat(r.Executable); err != nil {
		return fmt.Errorf("app path doesn't exist: %s", r.Executable)
	}
	file, err := agentFile(r.Name)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(file), 0o755); err != nil {
		return err
	}
	data := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
  <key>Label</key>
  <string>%s</string>
  <key>ProgramArguments</key>
  <array><string>%s</string><string>%s</string></array>
  <key>RunAtLoad</key>
  <true/>
  </dict>
</plist>`, html.EscapeString(r.Name), html.EscapeString(r.Executable), LaunchArg)
	return os.WriteFile(file, []byte(data), 0o644)
}

// Disable removes the LaunchAgent; a missing one is fine.
func (r Registration) Disable() error {
	file, err := agentFile(r.Name)
	if err != nil {
		return err
	}
	if err := os.Remove(file); err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return nil
}

// Enabled reports whether the LaunchAgent exists.
func (r Registration) Enabled() (bool, error) {
	file, err := agentFile(r.Name)
	if err != nil {
		return false, err
	}
	_, err = os.Stat(file)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	return err == nil, err
}

// Reconcile brings the OS registration in line with the desired preference.
func (r Registration) Reconcile(desired bool) error {
	if desired {
		return r.Enable()
	}
	enabled, err := r.Enabled()
	if err != nil || !enabled {
		return err
	}
	return r.Disable()
}
