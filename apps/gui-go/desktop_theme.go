package main

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

// desktopThemeState owns the palette and preference shared by every WebView.
// Only the preference is persisted; palettes are read from the desktop each time.
type desktopThemeState struct {
	mu      sync.Mutex
	path    string
	current DesktopThemeSnapshot
}

func newDesktopThemeState() *desktopThemeState {
	s := &desktopThemeState{current: DesktopThemeSnapshot{Revision: 1}}
	if runtime.GOOS != "linux" {
		return s
	}
	if root, ok := apppaths.AppDataRoot(); ok {
		s.path = filepath.Join(root, "desktop-theme.json")
		if raw, err := os.ReadFile(s.path); err == nil {
			var preference struct {
				Follow bool `json:"followOmarchyTheme"`
			}
			if json.Unmarshal(raw, &preference) == nil {
				s.current.FollowOmarchyTheme = preference.Follow
			}
		}
	}
	s.current.Theme, s.current.OmarchyAvailable = readOmarchyTheme()
	return s
}

func (s *desktopThemeState) snapshot() DesktopThemeSnapshot {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.current
}

func (s *desktopThemeState) setEnabled(enabled bool) (DesktopThemeSnapshot, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if !s.current.OmarchyAvailable {
		return s.current, nil
	}
	if s.path == "" {
		return s.current, errors.New("desktop theme configuration unavailable")
	}
	raw, err := json.Marshal(struct {
		Follow bool `json:"followOmarchyTheme"`
	}{enabled})
	if err != nil {
		return s.current, err
	}
	dir := filepath.Dir(s.path)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return s.current, err
	}
	f, err := os.CreateTemp(dir, ".desktop-theme-*")
	if err != nil {
		return s.current, err
	}
	defer os.Remove(f.Name())
	if _, err := f.Write(raw); err != nil {
		f.Close()
		return s.current, err
	}
	if err := f.Sync(); err != nil {
		f.Close()
		return s.current, err
	}
	if err := f.Close(); err != nil {
		return s.current, err
	}
	if err := replaceEffectsFile(f.Name(), s.path); err != nil {
		return s.current, err
	}
	s.current.FollowOmarchyTheme = enabled
	s.current.Revision++
	return s.current, nil
}

func (s *desktopThemeState) refresh() (DesktopThemeSnapshot, bool) {
	theme, available := readOmarchyTheme()
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.current.OmarchyAvailable == available && reflect.DeepEqual(s.current.Theme, theme) {
		return s.current, false
	}
	s.current.Theme, s.current.OmarchyAvailable = theme, available
	s.current.Revision++
	return s.current, true
}

func (h *HostService) startDesktopTheme() {
	if runtime.GOOS != "linux" {
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	h.stopDesktopTheme = cancel
	go func() {
		// Reopen the source rather than watching its inode: Omarchy replaces the
		// current theme directory, and older versions replace a symlink.
		ticker := time.NewTicker(time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				if snapshot, changed := h.desktopTheme.refresh(); changed {
					h.emit(desktopThemeChangedEvent, snapshot)
				}
			}
		}
	}()
}
