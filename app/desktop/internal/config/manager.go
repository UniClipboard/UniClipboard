package config

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"sync"
	"time"

	"github.com/fsnotify/fsnotify"

	"github.com/uniclipboard/unic/internal/plugins/builtintext"
)

const debounce = 150 * time.Millisecond

// State is a snapshot of the manager, for `unic config` and the API.
type State struct {
	Path   string `json:"path"`
	Config Config `json:"config"`
	// Error is why the last reload was rejected; Config is then the last
	// good one. Empty when the file on disk is what is in effect.
	Error string `json:"error,omitempty"`
	// RestartRequired lists keys whose new value waits for a restart.
	RestartRequired []string `json:"restart_required,omitempty"`
}

// Manager holds the current configuration and reloads it when the file changes.
type Manager struct {
	path string
	logf func(string, ...any)

	mu      sync.RWMutex
	cur     Config
	started Config // as of startup, for keys that need a restart
	err     error
	marked  map[string]bool // keys the host found to need a restart

	// OnChange, if set before Watch, is called after each reload attempt
	// (good or bad) with the state after it.
	OnChange func(old, cur Config, err error)
}

// Open reads path, creating it from the template first if absent. A file
// that is present but invalid is an error: at startup there is no previous
// configuration to fall back to.
func Open(path string, logf func(string, ...any)) (*Manager, error) {
	if err := WriteTemplate(path); err != nil {
		return nil, err
	}
	m := &Manager{path: path, logf: logf}
	c, err := m.read()
	if err != nil {
		return nil, err
	}
	m.cur, m.started = c, c
	return m, nil
}

// Static returns a manager that holds c and never changes (tests).
func Static(c Config) *Manager { return &Manager{cur: c, started: c} }

func (m *Manager) read() (Config, error) {
	b, err := os.ReadFile(m.path)
	if err != nil {
		return Config{}, err
	}
	return Parse(b)
}

// Current is the configuration in effect now.
func (m *Manager) Current() Config {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.cur
}

// State describes the manager.
func (m *Manager) State() State {
	m.mu.RLock()
	defer m.mu.RUnlock()
	s := State{Path: m.path, Config: m.cur}
	if m.err != nil {
		s.Error = m.err.Error()
	}
	if m.cur.Inbox != m.started.Inbox {
		s.RestartRequired = append(s.RestartRequired, "inbox")
	}
	if m.cur.History != m.started.History {
		s.RestartRequired = append(s.RestartRequired, "history")
	}
	if m.cur.PluginEnabled(builtintext.ID) != m.started.PluginEnabled(builtintext.ID) {
		s.RestartRequired = append(s.RestartRequired, "plugins")
	}
	if !reflect.DeepEqual(m.cur.processorSettingsKey(), m.started.processorSettingsKey()) {
		s.RestartRequired = append(s.RestartRequired, "plugins.settings")
	}
	if !slices.Equal(m.cur.Plugins.Lua, m.started.Plugins.Lua) {
		s.RestartRequired = append(s.RestartRequired, "plugins.lua")
	}
	for k := range m.marked {
		if !slices.Contains(s.RestartRequired, k) {
			s.RestartRequired = append(s.RestartRequired, k)
		}
	}
	slices.Sort(s.RestartRequired)
	return s
}

// MarkRestart records that a change the host cannot apply live is waiting
// for a restart.
func (m *Manager) MarkRestart(key string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.marked == nil {
		m.marked = map[string]bool{}
	}
	m.marked[key] = true
}

// Reload re-reads the file now. A bad file leaves the current configuration
// in place and returns the error.
func (m *Manager) Reload() error {
	c, err := m.read()
	m.mu.Lock()
	old := m.cur
	m.err = err
	if err == nil {
		m.cur = c
	}
	cur := m.cur
	m.mu.Unlock()
	if err != nil {
		m.logf("config: %s not applied: %v", m.path, err)
	} else if !reflect.DeepEqual(c, old) {
		m.logf("config: reloaded")
	}
	if m.OnChange != nil {
		m.OnChange(old, cur, err)
	}
	return err
}

// Watch reloads the file whenever it changes, until ctx is done. It watches
// the directory because editors often save by renaming a temporary file over
// the original, which a watch on the file itself would lose.
func (m *Manager) Watch(ctx context.Context) error {
	w, err := fsnotify.NewWatcher()
	if err != nil {
		return err
	}
	defer func() { _ = w.Close() }()
	if err := w.Add(filepath.Dir(m.path)); err != nil {
		return err
	}
	var timer *time.Timer
	var fire <-chan time.Time
	for {
		select {
		case <-ctx.Done():
			return nil
		case ev, ok := <-w.Events:
			if !ok {
				return nil
			}
			if filepath.Clean(ev.Name) != filepath.Clean(m.path) {
				continue
			}
			if timer == nil {
				timer = time.NewTimer(debounce)
			} else {
				if !timer.Stop() {
					select {
					case <-timer.C:
					default:
					}
				}
				timer.Reset(debounce)
			}
			fire = timer.C
		case <-fire:
			fire = nil
			_ = m.Reload()
		case err, ok := <-w.Errors:
			if !ok {
				return nil
			}
			m.logf("config: watch: %v", err)
		}
	}
}
