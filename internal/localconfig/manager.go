package config

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"

	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/BurntSushi/toml"

	"github.com/fsnotify/fsnotify"

	"github.com/uniclipboard/unic/internal/plugins/builtintext"
)

const debounce = 150 * time.Millisecond

// State is a snapshot of the manager, for `unic config` and the API.
type State struct {
	CacheScope string `json:"scope"`
	Revision   string `json:"revision"`
	ApplyError string `json:"apply_error,omitempty"`
	Path       string `json:"path"`
	Config     Config `json:"config"`
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

	ioMu       sync.Mutex
	raw        []byte
	applyError string
	mu         sync.RWMutex
	cur        Config
	started    Config // as of startup, for keys that need a restart
	err        error
	marked     map[string]bool // keys the host found to need a restart

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
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	c, err := Parse(raw)
	if err != nil {
		return nil, err
	}
	m.cur, m.started, m.raw = c, c, raw
	return m, nil
}

// Static returns a manager that holds c and never changes (tests).
func Static(c Config) *Manager { return &Manager{cur: c, started: c} }

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
	sum := sha256.Sum256(m.raw)
	s := State{CacheScope: "default_directory_only", Path: m.path, Config: m.cur, Revision: hex.EncodeToString(sum[:]), ApplyError: m.applyError}
	if m.err != nil {
		s.Error = m.err.Error()
	}
	if m.cur.Telemetry.Endpoint != m.started.Telemetry.Endpoint {
		s.RestartRequired = append(s.RestartRequired, "telemetry.endpoint")
	}
	if m.cur.Inbox != m.started.Inbox {
		s.RestartRequired = append(s.RestartRequired, "inbox")
	}
	if m.cur.History.MaxBytes != m.started.History.MaxBytes {
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
// ErrConflict means that the caller must read the configuration again.
var ErrConflict = errors.New("configuration changed; read it again")

func (m *Manager) ApplyFailed(err error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.applyError = ""
	if err != nil {
		m.applyError = "saved settings could not be applied"
	}
}

func (m *Manager) Reload() error {
	m.ioMu.Lock()
	defer m.ioMu.Unlock()
	raw, err := os.ReadFile(m.path)
	var c Config
	if err == nil {
		c, err = Parse(raw)
	}
	m.publish(raw, c, err)
	return err
}

// publish calls OnChange without holding the state lock. ioMu serializes writes.
func (m *Manager) publish(raw []byte, c Config, err error) {
	m.mu.Lock()
	old := m.cur
	m.err = err
	if raw != nil {
		m.raw = bytes.Clone(raw)
	}
	if err == nil {
		m.cur = c
	}
	cur := m.cur
	m.mu.Unlock()
	if m.OnChange != nil {
		m.OnChange(old, cur, err)
	}
}

// Replace validates a complete TOML document and atomically replaces the file.
// External editors do not honor ioMu; the watcher imports later external saves.
func (m *Manager) Replace(revision string, raw []byte) (State, error) {
	c, err := Parse(raw)
	if err != nil {
		return State{}, err
	}
	m.ioMu.Lock()
	defer m.ioMu.Unlock()
	if revision != m.State().Revision {
		return State{}, ErrConflict
	}
	disk, err := os.ReadFile(m.path)
	if err != nil {
		return State{}, errors.New("cannot read configuration file")
	}
	m.mu.RLock()
	same := bytes.Equal(disk, m.raw)
	m.mu.RUnlock()
	if !same {
		return State{}, ErrConflict
	}
	f, err := os.CreateTemp(filepath.Dir(m.path), ".unic-config-*")
	if err != nil {
		return State{}, errors.New("cannot stage configuration file")
	}
	defer func() { _ = os.Remove(f.Name()) }()
	if _, err = f.Write(raw); err == nil {
		err = f.Sync()
	}
	if cerr := f.Close(); err == nil {
		err = cerr
	}
	if err == nil {
		err = os.Rename(f.Name(), m.path)
	}
	if err != nil {
		return State{}, errors.New("cannot save configuration file")
	}
	m.publish(raw, c, nil)
	return m.State(), nil
}

// Set updates one dotted key using TOML value syntax and preserves other values.
func Set(c Config, key, value string) ([]byte, error) {
	var buf bytes.Buffer
	if err := toml.NewEncoder(&buf).Encode(c); err != nil {
		return nil, err
	}
	var fields map[string]any
	if _, err := toml.Decode(buf.String(), &fields); err != nil {
		return nil, err
	}
	var input struct{ Value any }
	if _, err := toml.Decode("value = "+value, &input); err != nil {
		return nil, errors.New("value must use TOML syntax; quote strings")
	}
	parts := strings.Split(key, ".")
	m := fields
	for _, k := range parts[:len(parts)-1] {
		next, ok := m[k].(map[string]any)
		if !ok {
			return nil, errors.New("unknown configuration key")
		}
		m = next
	}
	k := parts[len(parts)-1]
	if _, ok := m[k]; !ok {
		return nil, errors.New("unknown configuration key")
	}
	m[k] = input.Value
	buf.Reset()
	if err := toml.NewEncoder(&buf).Encode(fields); err != nil {
		return nil, err
	}
	if _, err := Parse(buf.Bytes()); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
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
