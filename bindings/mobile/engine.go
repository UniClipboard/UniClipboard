package mobile

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime/debug"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/skip2/go-qrcode"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/plugins/builtinclassify"
	"github.com/uniclipboard/unic/internal/plugins/builtinpreview"
	"github.com/uniclipboard/unic/internal/plugins/builtintext"
	"github.com/uniclipboard/unic/internal/plugins/tailcat"
	"github.com/uniclipboard/unic/internal/procmetrics"
	"github.com/uniclipboard/unic/internal/relaycrypto"
)

// EventSink receives asynchronous Engine state change notifications.
type EventSink interface {
	OnEvent(eventJSON string)
	OnResyncRequired(reason string)
}

// Engine wraps *engine.Engine behind a gomobile-safe, JSON-typed boundary.
type Engine struct {
	e                    *engine.Engine
	dataDir              string
	settingsPath         string
	relaySecretPath      string
	cacheDir             string
	extensionMode        bool
	settingsMu           sync.Mutex
	storedSettings       engine.Settings
	relayURL             string
	relayAccessKey       string
	relayAccessKeyTarget string
	relayAccessKeyBound  bool
	cloudRelay           *mobileCloudRelay
	previewMu            sync.Mutex
	diagnostics          *mobileDiagnostics
	resourceMu           sync.RWMutex
	resourceProvider     HostResourceProvider
	fallbackSamplerMu    sync.Mutex
	fallbackSampler      *procmetrics.Sampler
	fallbackEpoch        uint64

	subMu     sync.Mutex
	sub       *engine.Subscription
	subCancel context.CancelFunc
	subWG     sync.WaitGroup
}

type engineConfigDTO struct {
	DataDir          string                   `json:"data_dir"`
	CacheDir         string                   `json:"cache_dir,omitempty"`
	DeviceName       string                   `json:"device_name,omitempty"`
	RendezvousURL    string                   `json:"rendezvous_url,omitempty"`
	RelayEnabled     *bool                    `json:"relay_enabled,omitempty"`
	RelayURL         string                   `json:"relay_url,omitempty"`
	RelayAccessKey   string                   `json:"relay_access_key,omitempty"`
	RelayMaxBytes    int64                    `json:"relay_max_bytes,omitempty"`
	HostCapabilities *engine.HostCapabilities `json:"host_capabilities,omitempty"`
	ExtensionMode    bool                     `json:"extension_mode,omitempty"`
}

// ConfigureRuntimeMemory tunes the Go GC target and soft memory limit for
// memory-constrained host processes such as the iOS Keyboard Extension.
// Non-positive values leave the corresponding runtime setting unchanged.
func ConfigureRuntimeMemory(gcPercent int, memLimitMB int) {
	if gcPercent > 0 {
		debug.SetGCPercent(gcPercent)
	}
	if memLimitMB > 0 {
		debug.SetMemoryLimit(int64(memLimitMB) * 1024 * 1024)
	}
}

// FreeOSMemory forces a garbage collection and returns unused memory to the OS.
func FreeOSMemory() {
	debug.FreeOSMemory()
}

func resolveDirPath(path string) (string, error) {
	if strings.TrimSpace(path) == "" {
		return "", errors.New("empty path")
	}
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	var tail []string
	cur := abs
	for {
		resolved, err := filepath.EvalSymlinks(cur)
		if err == nil {
			for i := len(tail) - 1; i >= 0; i-- {
				resolved = filepath.Join(resolved, tail[i])
			}
			return resolved, nil
		}
		if !os.IsNotExist(err) {
			return abs, nil
		}
		parent := filepath.Dir(cur)
		if parent == cur {
			return abs, nil
		}
		tail = append(tail, filepath.Base(cur))
		cur = parent
	}
}

const mobileSettingsFilename = "mobile-settings.json"

func loadSettingsOrDefault(path string, cfg engineConfigDTO) (engine.Settings, error) {
	def := engine.DefaultSettings()
	raw, err := os.ReadFile(path)
	if err == nil {
		saved := def
		if uerr := json.Unmarshal(raw, &saved); uerr == nil {
			if saved.Limits.MaxFileBytes == 0 {
				saved.Limits.MaxFileBytes = def.Limits.MaxFileBytes
			}
			if saved.Limits.MaxTotalBytes == 0 {
				saved.Limits.MaxTotalBytes = def.Limits.MaxTotalBytes
			}
			if saved.DeviceName == "" && strings.TrimSpace(cfg.DeviceName) != "" {
				saved.DeviceName = strings.TrimSpace(cfg.DeviceName)
			}
			if saved.RendezvousURL == "" && strings.TrimSpace(cfg.RendezvousURL) != "" {
				saved.RendezvousURL = strings.TrimSpace(cfg.RendezvousURL)
			}
			applyRelayConfigDefaults(&saved, cfg)
			return saved, nil
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return def, err
	}
	if name := strings.TrimSpace(cfg.DeviceName); name != "" {
		def.DeviceName = name
	}
	if u := strings.TrimSpace(cfg.RendezvousURL); u != "" {
		def.RendezvousURL = u
	}
	applyRelayConfigDefaults(&def, cfg)
	return def, nil
}

func applyRelayConfigDefaults(s *engine.Settings, cfg engineConfigDTO) {
	if s.Sync.Relay.URL == "" {
		if u := strings.TrimSpace(cfg.RelayURL); u != "" {
			s.Sync.Relay.URL = u
		}
	}
	if s.Sync.Relay.AccessKey == "" {
		if k := strings.TrimSpace(cfg.RelayAccessKey); k != "" {
			sTarget, sOK := canonicalRelayTarget(s.Sync.Relay.URL)
			cfgTarget, cfgOK := canonicalRelayTarget(cfg.RelayURL)
			if sOK && cfgOK && sTarget == cfgTarget {
				s.Sync.Relay.AccessKey = k
			}
		}
	}
	if s.Sync.Relay.MaxBytes == 0 && cfg.RelayMaxBytes > 0 {
		s.Sync.Relay.MaxBytes = cfg.RelayMaxBytes
	}
	if cfg.RelayEnabled != nil && !s.Sync.Relay.Enabled && s.Sync.Relay.URL != "" {
		s.Sync.Relay.Enabled = *cfg.RelayEnabled
	}
}

type mobileRelaySecretRecord struct {
	Target    *string `json:"target"`
	AccessKey string  `json:"access_key"`
}

func canonicalRelayTarget(raw string) (string, bool) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return "", true
	}
	norm, err := relaycrypto.NormalizeRelayOrigin(trimmed)
	if err != nil {
		return "", false
	}
	return norm, true
}

func loadRelaySecretForTarget(path, expectedURL string) (string, bool, bool) {
	raw, err := os.ReadFile(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return "", false, false
		}
		return "", false, true
	}
	wantTarget, ok := canonicalRelayTarget(expectedURL)
	if !ok {
		return "", false, true
	}
	var rec mobileRelaySecretRecord
	if err := json.Unmarshal(raw, &rec); err != nil {
		return "", false, true
	}
	if rec.Target == nil {
		return "", false, true
	}
	recTarget, ok := canonicalRelayTarget(*rec.Target)
	if !ok || recTarget != wantTarget {
		return "", false, true
	}
	key := strings.TrimSpace(rec.AccessKey)
	if key == "" {
		return "", false, true
	}
	return key, true, true
}

func saveRelaySecretAtomic(path, targetURL, key string) error {
	key = strings.TrimSpace(key)
	if key == "" {
		if err := os.Remove(path); err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
		}
		return nil
	}
	normTarget, ok := canonicalRelayTarget(targetURL)
	if !ok {
		return errors.New("invalid relay secret target")
	}
	raw, err := json.Marshal(mobileRelaySecretRecord{
		Target:    &normTarget,
		AccessKey: key,
	})
	if err != nil {
		return err
	}
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".mobile-relay-secret-*")
	if err != nil {
		return err
	}
	tmpName := tmp.Name()
	defer func() { _ = os.Remove(tmpName) }()
	if err := tmp.Chmod(0o600); err != nil {
		_ = tmp.Close()
		return err
	}
	if _, err := tmp.Write(append(raw, '\n')); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmpName, path)
}

func saveSettingsAtomic(path string, s any) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	raw, err := json.MarshalIndent(s, "", "  ")
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".mobile-settings-*")
	if err != nil {
		return err
	}
	tmpName := tmp.Name()
	defer func() { _ = os.Remove(tmpName) }()
	if _, err := tmp.Write(append(raw, '\n')); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmpName, path)
}

func marshalJSON(v any) (string, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return "", wrapErr(err)
	}
	return string(b), nil
}

func parseUint64Flexible(raw json.RawMessage) (uint64, error) {
	s := strings.TrimSpace(string(raw))
	if s == "" || s == "null" {
		return 0, nil
	}
	if strings.HasPrefix(s, `"`) {
		var str string
		if err := json.Unmarshal(raw, &str); err != nil {
			return 0, err
		}
		str = strings.TrimSpace(str)
		if str == "" {
			return 0, nil
		}
		return strconv.ParseUint(str, 10, 64)
	}
	return strconv.ParseUint(s, 10, 64)
}

// NewEngine creates and initializes an in-process Engine instance without starting networking.
func NewEngine(configJSON string) (*Engine, error) {
	return newEngineWithBackend(configJSON, tailcat.New())
}

func newEngineWithBackend(configJSON string, peer *tailcat.Plugin) (*Engine, error) {
	return newEngineWithOptions(configJSON, peer, nil, nil)
}

func newEngineWithOptions(configJSON string, peer *tailcat.Plugin, cloudHTTP *http.Client, nowFn func() time.Time) (*Engine, error) {
	var cfg engineConfigDTO
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return nil, wrapErr(errors.New("invalid engine config json"))
	}
	dataDir, err := resolveDirPath(cfg.DataDir)
	if err != nil {
		return nil, wrapErr(err)
	}
	if err := os.MkdirAll(dataDir, 0o700); err != nil {
		return nil, wrapErr(err)
	}
	// Resolve again after creation so DataDir/CacheDir never fail openFileCache's symlink guard.
	dataDir, _ = resolveDirPath(dataDir)

	cacheDir := cfg.CacheDir
	if strings.TrimSpace(cacheDir) == "" {
		cacheDir = filepath.Join(dataDir, "inbox", "file-cache")
	}
	if strings.TrimSpace(cacheDir) != "" {
		if resolved, rerr := resolveDirPath(cacheDir); rerr == nil {
			cacheDir = resolved
		}
	}

	settingsPath := filepath.Join(dataDir, mobileSettingsFilename)
	relaySecretPath := filepath.Join(dataDir, "mobile-relay-secret")
	storedSettings, err := loadSettingsOrDefault(settingsPath, cfg)
	if err != nil {
		return nil, wrapErr(err)
	}
	wasHasAccessKey := storedSettings.Sync.Relay.HasAccessKey
	inlineKey := strings.TrimSpace(storedSettings.Sync.Relay.AccessKey)
	storedSettings.Sync.Relay.AccessKey = ""

	var manualKey string
	var manualKeyTarget string
	var manualKeyBound bool
	persistedKey, proven, secretFilePresent := loadRelaySecretForTarget(relaySecretPath, storedSettings.Sync.Relay.URL)
	if proven {
		manualKey = persistedKey
		manualKeyTarget, manualKeyBound = canonicalRelayTarget(storedSettings.Sync.Relay.URL)
	} else {
		if secretFilePresent {
			// Unattributed legacy or interrupted-write mismatched secret cannot be proven
			// to belong to the loaded target; reject and remove it before starting Engine.
			_ = saveRelaySecretAtomic(relaySecretPath, "", "")
		}
		if inlineKey != "" {
			if normTarget, ok := canonicalRelayTarget(storedSettings.Sync.Relay.URL); ok {
				manualKey = inlineKey
				manualKeyTarget = normTarget
				manualKeyBound = true
			}
		}
	}
	storedSettings.Sync.Relay.HasAccessKey = manualKey != "" && manualKeyBound

	caps := engine.HostCapabilities{
		Observe: engine.AbilityUnavailable,
		Apply:   engine.AbilityUnavailable,
	}
	if cfg.HostCapabilities != nil {
		caps = *cfg.HostCapabilities
	}

	if peer == nil {
		peer = tailcat.New()
	}

	classifier, err := builtinclassify.New(nil, nil)
	if err != nil {
		return nil, wrapErr(err)
	}
	plugins := []engine.PluginInstance{
		{Plugin: peer, Builtin: true},
		{Plugin: classifier},
		{Plugin: builtintext.New()},
	}
	if !cfg.ExtensionMode {
		previewer, err := builtinpreview.New(builtinpreview.Settings{})
		if err != nil {
			return nil, wrapErr(err)
		}
		plugins = append(plugins, engine.PluginInstance{Plugin: previewer, Builtin: true})
	}

	m := &Engine{
		dataDir:              dataDir,
		settingsPath:         settingsPath,
		relaySecretPath:      relaySecretPath,
		cacheDir:             cacheDir,
		extensionMode:        cfg.ExtensionMode,
		storedSettings:       storedSettings,
		relayURL:             storedSettings.Sync.Relay.URL,
		relayAccessKey:       manualKey,
		relayAccessKeyTarget: manualKeyTarget,
		relayAccessKeyBound:  manualKeyBound,
		fallbackSampler:      procmetrics.NewSampler(),
	}
	m.cloudRelay = newMobileCloudRelay(m, dataDir, cloudHTTP, nowFn)
	_, _, _ = m.cloudRelay.mgr.Reload()
	needStartupSave := wasHasAccessKey != (manualKey != "" && manualKeyBound)
	if m.cloudRelay.shouldDisableOfficialRelay(m.storedSettings, m.relayAccessKey) {
		m.storedSettings.Sync.Relay.Enabled = false
		needStartupSave = true
	}
	if needStartupSave {
		_ = saveSettingsAtomic(settingsPath, m.redactedStoredSettingsLocked())
	}
	m.cloudRelay.setStoredConfig(m.storedSettings, m.relayAccessKey, m.relayAccessKeyTarget, m.relayAccessKeyBound)
	guardedSettings := m.cloudRelay.applyRelayGuard(m.storedSettings, m.relayAccessKey)

	diagnostics := newMobileDiagnostics(dataDir)
	peer.Observe(diagnostics.controller)
	opts := engine.Options{
		DataDir:          dataDir,
		CacheDir:         cacheDir,
		Settings:         guardedSettings,
		HostCapabilities: caps,
		Logger:           diagnostics.controller.Logger(),
		Diagnostics:      diagnostics.controller.Record,
		ProductEvents:    diagnostics.controller.ProductEvent,
		Tracer:           diagnostics.controller,
		Plugins:          plugins,
		RelayAuth:        m.cloudRelay,
	}

	eng, err := engine.New(context.Background(), opts)
	if err != nil {
		diagnostics.close(context.Background())
		return nil, wrapErr(err)
	}
	diagnostics.controller.SetSyncCollector(peer.Collect)
	m.e = eng
	m.diagnostics = diagnostics
	diagnostics.controller.SetResourceCollector(m.collectResources)
	return m, nil
}

// Start brings up local services and networking (if runtime state is active).
func (m *Engine) Start(token *CancelToken) error {
	ctx, cancel := withToken(token)
	defer cancel()
	if err := m.e.Start(ctx); err != nil {
		return wrapErr(err)
	}
	if m.cloudRelay != nil {
		m.cloudRelay.start()
	}
	if m.extensionMode {
		m.diagnostics.controller.EnableProduct("mobile")
	} else {
		m.diagnostics.controller.StartProduct("mobile")
	}
	return nil
}

// Stop shuts down networking and waits for active sessions to exit.
func (m *Engine) Stop(token *CancelToken) error {
	if m.cloudRelay != nil {
		m.cloudRelay.stop()
	}
	ctx, cancel := withToken(token)
	defer cancel()
	return wrapErr(m.e.Stop(ctx))
}

func (m *Engine) stopSubscription() {
	m.subMu.Lock()
	sub := m.sub
	cancel := m.subCancel
	m.sub = nil
	m.subCancel = nil
	m.subMu.Unlock()
	if cancel != nil {
		cancel()
	}
	if sub != nil {
		_ = sub.Close()
	}
	m.subWG.Wait()
}

// Close stops event subscriptions, shuts down Engine, and releases all storage locks.
func (m *Engine) Close(token *CancelToken) error {
	m.stopSubscription()
	if m.cloudRelay != nil {
		m.cloudRelay.stop()
	}
	ctx, cancel := withToken(token)
	defer cancel()
	err := m.e.Close(ctx)
	m.diagnostics.close(ctx)
	return wrapErr(err)
}

// SetRuntimeState sets Engine runtime networking permission ("active" or "suspended").
func (m *Engine) SetRuntimeState(state string) error {
	return wrapErr(m.e.SetRuntimeState(context.Background(), engine.RuntimeState(state)))
}

// NotifyNetworkChanged asks the network backend to re-evaluate interfaces and reset backoff.
func (m *Engine) NotifyNetworkChanged() error {
	if m.cloudRelay != nil {
		m.cloudRelay.notify()
	}
	return wrapErr(m.e.NotifyNetworkChanged(context.Background()))
}

// UpdateHostCapabilities updates platform clipboard read/write capabilities.
func (m *Engine) UpdateHostCapabilities(capsJSON string) error {
	var caps engine.HostCapabilities
	if err := json.Unmarshal([]byte(capsJSON), &caps); err != nil {
		return wrapErr(errors.New("invalid capabilities json"))
	}
	return wrapErr(m.e.UpdateHostCapabilities(context.Background(), caps))
}

func (m *Engine) effectiveManualKeyForLocked(relayURL string) (string, bool) {
	k := strings.TrimSpace(m.relayAccessKey)
	if k == "" || !m.relayAccessKeyBound {
		return "", false
	}
	target, ok := canonicalRelayTarget(relayURL)
	if !ok || target != m.relayAccessKeyTarget {
		return "", false
	}
	return k, true
}

func (m *Engine) redactedStoredSettingsLocked() engine.Settings {
	out := m.storedSettings
	out.Sync.Relay.AccessKey = ""
	_, hasKey := m.effectiveManualKeyForLocked(out.Sync.Relay.URL)
	out.Sync.Relay.HasAccessKey = hasKey
	return out
}

func (m *Engine) projectSettingsState(st engine.SettingsState) engine.SettingsState {
	m.settingsMu.Lock()
	relay := m.storedSettings.Sync.Relay
	relay.AccessKey = ""
	_, hasKey := m.effectiveManualKeyForLocked(relay.URL)
	relay.HasAccessKey = hasKey
	m.settingsMu.Unlock()
	st.Value.Sync.Relay = relay
	return st
}

func (m *Engine) projectState(st engine.State) engine.State {
	st.Settings = m.projectSettingsState(st.Settings)
	return st
}

// GetState returns the current secret-free State snapshot as JSON.
func (m *Engine) GetState() (string, error) {
	st, err := m.e.GetState(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(m.projectState(st))
}

// GetCapabilities returns the declared, core, and effective capabilities as JSON.
func (m *Engine) GetCapabilities() (string, error) {
	caps, err := m.e.GetCapabilities(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(caps)
}

type updateSettingsDTO struct {
	ExpectedRevision json.RawMessage `json:"expected_revision"`
	Value            engine.Settings `json:"value"`
	// RelayAccessKey is write-only: nil preserves existing key, "" clears it, non-empty updates it.
	RelayAccessKey *string `json:"relay_access_key,omitempty"`
}

func normalizedRelayTarget(raw string) string {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return ""
	}
	if norm, err := relaycrypto.NormalizeRelayOrigin(trimmed); err == nil {
		return norm
	}
	return trimmed
}

func (m *Engine) rollbackSettingsLocked(currentRev uint64, prevStored engine.Settings, prevManualKey, prevManualTarget string, prevManualBound bool) {
	prevGuarded := prevStored
	if m.cloudRelay != nil {
		m.cloudRelay.setStoredConfig(prevStored, prevManualKey, prevManualTarget, prevManualBound)
		prevGuarded = m.cloudRelay.applyRelayGuard(prevStored, prevManualKey)
	} else {
		prevGuarded.Sync.Relay.AccessKey = prevManualKey
	}
	_, _ = m.e.UpdateSettings(context.Background(), engine.UpdateSettingsRequest{
		ExpectedRevision: currentRev,
		Value:            prevGuarded,
	})
}

// UpdateSettings validates, updates, and persists Engine settings.
func (m *Engine) UpdateSettings(reqJSON string) (string, error) {
	var req updateSettingsDTO
	if err := json.Unmarshal([]byte(reqJSON), &req); err != nil {
		return "", wrapErr(errors.New("invalid update settings json"))
	}
	rev, err := parseUint64Flexible(req.ExpectedRevision)
	if err != nil {
		return "", wrapErr(errors.New("invalid expected_revision"))
	}

	m.settingsMu.Lock()
	prevStored := m.storedSettings
	prevManualTarget := m.relayAccessKeyTarget
	prevManualKey, prevManualBound := m.effectiveManualKeyForLocked(prevStored.Sync.Relay.URL)
	prevTarget := normalizedRelayTarget(prevStored.Sync.Relay.URL)
	nextTarget := normalizedRelayTarget(req.Value.Sync.Relay.URL)
	urlChanged := prevTarget != nextTarget

	var nextManualKey string
	switch {
	case req.RelayAccessKey != nil:
		nextManualKey = strings.TrimSpace(*req.RelayAccessKey)
	case strings.TrimSpace(req.Value.Sync.Relay.AccessKey) != "":
		nextManualKey = strings.TrimSpace(req.Value.Sync.Relay.AccessKey)
	case urlChanged:
		// Clear old manual credential before the new relay URL can issue requests.
		nextManualKey = ""
	default:
		nextManualKey = prevManualKey
	}
	nextManualTarget, nextValidTarget := canonicalRelayTarget(req.Value.Sync.Relay.URL)
	nextManualBound := nextManualKey != "" && nextValidTarget

	nextStored := req.Value
	nextStored.Sync.Relay.URL = strings.TrimSpace(nextStored.Sync.Relay.URL)
	nextStored.Sync.Relay.AccessKey = ""
	nextStored.Sync.Relay.HasAccessKey = nextManualBound

	if m.cloudRelay != nil && m.cloudRelay.shouldDisableOfficialRelay(nextStored, nextManualKey) {
		nextStored.Sync.Relay.Enabled = false
	}

	guarded := nextStored
	if m.cloudRelay != nil {
		guarded = m.cloudRelay.applyRelayGuard(nextStored, nextManualKey)
		// Publish updated target/key decision to RelayAuth before Engine reconciles
		// control-plane and SSE loops inside m.e.UpdateSettings.
		m.cloudRelay.setStoredConfig(nextStored, nextManualKey, nextManualTarget, nextManualBound)
	} else {
		guarded.Sync.Relay.AccessKey = nextManualKey
	}

	st, err := m.e.UpdateSettings(context.Background(), engine.UpdateSettingsRequest{
		ExpectedRevision: rev,
		Value:            guarded,
	})
	if err != nil {
		if m.cloudRelay != nil {
			m.cloudRelay.setStoredConfig(prevStored, prevManualKey, prevManualTarget, prevManualBound)
			_ = m.cloudRelay.applyRelayGuard(prevStored, prevManualKey)
		}
		m.settingsMu.Unlock()
		return "", wrapErr(err)
	}

	storedSnapshot := st.Value
	storedSnapshot.Sync.Relay = nextStored.Sync.Relay
	storedSnapshot.Sync.Relay.AccessKey = ""
	storedSnapshot.Sync.Relay.HasAccessKey = nextManualBound

	needSecretSave := req.RelayAccessKey != nil || nextManualKey != prevManualKey || (nextManualBound && prevManualTarget != nextManualTarget)
	if needSecretSave {
		if kerr := saveRelaySecretAtomic(m.relaySecretPath, nextStored.Sync.Relay.URL, nextManualKey); kerr != nil {
			m.rollbackSettingsLocked(st.Revision, prevStored, prevManualKey, prevManualTarget, prevManualBound)
			m.settingsMu.Unlock()
			return "", wrapErr(kerr)
		}
	}
	if serr := saveSettingsAtomic(m.settingsPath, storedSnapshot); serr != nil {
		if needSecretSave {
			_ = saveRelaySecretAtomic(m.relaySecretPath, prevStored.Sync.Relay.URL, prevManualKey)
		}
		m.rollbackSettingsLocked(st.Revision, prevStored, prevManualKey, prevManualTarget, prevManualBound)
		m.settingsMu.Unlock()
		return "", wrapErr(serr)
	}

	m.storedSettings = storedSnapshot
	m.relayURL = storedSnapshot.Sync.Relay.URL
	m.relayAccessKey = nextManualKey
	m.relayAccessKeyTarget = nextManualTarget
	m.relayAccessKeyBound = nextManualBound
	if m.cloudRelay != nil {
		m.cloudRelay.setStoredConfig(m.storedSettings, m.relayAccessKey, m.relayAccessKeyTarget, m.relayAccessKeyBound)
	}

	projected := st
	projected.Value = m.redactedStoredSettingsLocked()
	m.settingsMu.Unlock()

	if m.cloudRelay != nil {
		m.cloudRelay.notify()
	}
	m.e.NotifyRelayAuthChanged()
	return marshalJSON(projected)
}

// ActiveSendCount returns the number of in-flight send operations so native
// iOS/Android hosts can grant a short background drain window before suspending.
func (m *Engine) ActiveSendCount() int {
	ops, err := m.e.ListOperations(context.Background())
	if err != nil {
		return 0
	}
	n := 0
	for _, op := range ops {
		if op.Kind == engine.OperationSend && !op.Done() {
			n++
		}
	}
	return n
}

// HasActiveSends reports whether any send operation is currently in flight.
func (m *Engine) HasActiveSends() bool { return m.ActiveSendCount() > 0 }

// Subscribe registers an EventSink and returns the initial State snapshot as JSON.
func (m *Engine) Subscribe(sink EventSink) (string, error) {
	m.stopSubscription()
	sub, err := m.e.SubscribeEvents(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	snap := m.projectState(sub.Snapshot())
	snapJSON, jerr := marshalJSON(snap)
	if jerr != nil {
		_ = sub.Close()
		return "", jerr
	}
	ctx, cancel := context.WithCancel(context.Background())
	m.subMu.Lock()
	m.sub = sub
	m.subCancel = cancel
	m.subWG.Add(1)
	m.subMu.Unlock()

	go func() {
		defer m.subWG.Done()
		for {
			ev, err := sub.Next(ctx)
			if err != nil {
				var ee *engine.Error
				if errors.As(err, &ee) && ee.Code == engine.CodeResyncRequired {
					if sink != nil {
						sink.OnResyncRequired("events_overflow")
					}
				}
				return
			}
			if sink != nil {
				if b, merr := json.Marshal(ev); merr == nil {
					sink.OnEvent(string(b))
				}
			}
		}
	}()

	return snapJSON, nil
}

// ---- Content & Streaming ----

// ImportText imports UTF-8 plain text (and optional HTML) into Engine and returns a ContentHandle JSON.
func (m *Engine) ImportText(text string, html string, obsKey string) (string, error) {
	var reps []engine.RepresentationInput
	if text != "" {
		b := []byte(text)
		reps = append(reps, engine.RepresentationInput{
			MIME:   engine.MimeText,
			Size:   int64(len(b)),
			Reader: bytes.NewReader(b),
		})
	}
	if html != "" {
		b := []byte(html)
		reps = append(reps, engine.RepresentationInput{
			MIME:   engine.MimeHTML,
			Size:   int64(len(b)),
			Reader: bytes.NewReader(b),
		})
	}
	h, err := m.e.ImportContent(context.Background(), engine.ContentInput{
		ObservationKey:  obsKey,
		Representations: reps,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(h)
}

// ImportBytes imports either a representation (when fileName is empty) or an owned file (when fileName is non-empty).
func (m *Engine) ImportBytes(mime string, fileName string, data []byte) (string, error) {
	in := engine.ContentInput{}
	if strings.TrimSpace(fileName) == "" {
		in.Representations = []engine.RepresentationInput{{
			MIME:   mime,
			Size:   int64(len(data)),
			Reader: bytes.NewReader(data),
		}}
	} else {
		in.Files = []engine.FileInput{{
			Name:   fileName,
			MIME:   mime,
			Size:   int64(len(data)),
			Reader: bytes.NewReader(data),
		}}
	}
	h, err := m.e.ImportContent(context.Background(), in)
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(h)
}

// ImportFileSource imports a file from a host FileOpener, copying into Engine storage or keeping by reference.
func (m *Engine) ImportFileSource(displayName string, mime string, copyIntoEngine bool, opener FileOpener) (string, error) {
	if opener == nil {
		return "", wrapErr(errors.New("file opener is nil"))
	}
	fi := engine.FileInput{
		Name: displayName,
		MIME: mime,
		Size: opener.Size(),
	}
	var opened io.ReadCloser
	if copyIntoEngine {
		stream, err := opener.Open()
		if err != nil {
			return "", wrapErr(err)
		}
		opened = &bridgedStreamReader{stream: stream}
		defer func() { _ = opened.Close() }()
		fi.Reader = opened
	} else {
		fi.Source = &bridgedFileSource{opener: opener}
	}
	h, err := m.e.ImportContent(context.Background(), engine.ContentInput{
		Files: []engine.FileInput{fi},
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(h)
}

// RetainContent creates a new lease for live content ID.
func (m *Engine) RetainContent(contentID string) (string, error) {
	h, err := m.e.RetainContent(context.Background(), engine.ContentID(contentID))
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(h)
}

// RetainHistoryContent opens a persisted history item for reading and returns a ContentHandle JSON.
func (m *Engine) RetainHistoryContent(historyID string) (string, error) {
	h, err := m.e.RetainHistoryContent(context.Background(), engine.HistoryID(historyID))
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(h)
}

// RetainHistoryForSend opens a persisted history item for explicit resending while enforcing processor sync policies.
func (m *Engine) RetainHistoryForSend(historyID string) (string, error) {
	h, err := m.e.RetainHistoryForSend(context.Background(), engine.HistoryID(historyID))
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(h)
}

// OpenPart opens a leased part for chunked streaming.
func (m *Engine) OpenPart(leaseID string, partID string) (*ContentStream, error) {
	r, err := m.e.ReadContent(context.Background(), engine.LeaseID(leaseID), engine.PartID(partID))
	if err != nil {
		return nil, wrapErr(err)
	}
	return &ContentStream{reader: r}, nil
}

// ExportPartToFile streams a leased part directly into destPath.
func (m *Engine) ExportPartToFile(leaseID string, partID string, destPath string) (int64, error) {
	st, err := m.OpenPart(leaseID, partID)
	if err != nil {
		return 0, err
	}
	return st.WriteToFile(destPath)
}

// ReadLeasedText reads bounded UTF-8 text from an already-leased part without releasing the caller's lease.
func (m *Engine) ReadLeasedText(leaseID string, partID string, maxBytes int) (string, error) {
	if maxBytes <= 0 {
		maxBytes = 2 << 20
	}
	if maxBytes > 16<<20 {
		maxBytes = 16 << 20
	}
	rd, err := m.e.ReadContent(context.Background(), engine.LeaseID(leaseID), engine.PartID(partID))
	if err != nil {
		return "", wrapErr(err)
	}
	text, trunc, readErr := readBoundedUTF8(rd, maxBytes)
	_ = rd.Close()
	if readErr != nil {
		return "", wrapErr(readErr)
	}
	return marshalJSON(textPreviewDTO{
		Text:      text,
		Truncated: trunc,
	})
}

type textPreviewDTO struct {
	Text      string `json:"text"`
	HTML      string `json:"html,omitempty"`
	Truncated bool   `json:"truncated"`
}

func readBoundedUTF8(r io.Reader, maxBytes int) (string, bool, error) {
	if maxBytes <= 0 {
		maxBytes = 8192
	}
	data, err := io.ReadAll(io.LimitReader(r, int64(maxBytes)+1))
	if err != nil {
		return "", false, err
	}
	truncated := len(data) > maxBytes
	if truncated {
		data = data[:maxBytes]
		for len(data) > 0 && !utf8.Valid(data) {
			data = data[:len(data)-1]
		}
	}
	return string(data), truncated, nil
}

// ReadHistoryTextPreview reads bounded text/plain and optional text/html previews and immediately releases its lease.
func (m *Engine) ReadHistoryTextPreview(historyID string, maxBytes int) (string, error) {
	h, err := m.e.RetainHistoryContent(context.Background(), engine.HistoryID(historyID))
	if err != nil {
		return "", wrapErr(err)
	}
	defer func() { _ = m.e.ReleaseContent(context.Background(), h.LeaseID) }()

	out := textPreviewDTO{}
	for _, p := range h.Content.Parts {
		if p.Kind != engine.PartRepresentation {
			continue
		}
		if p.MIME != engine.MimeText && p.MIME != engine.MimeHTML {
			continue
		}
		rd, rerr := m.e.ReadContent(context.Background(), h.LeaseID, p.ID)
		if rerr != nil {
			return "", wrapErr(rerr)
		}
		text, trunc, readErr := readBoundedUTF8(rd, maxBytes)
		_ = rd.Close()
		if readErr != nil {
			return "", wrapErr(readErr)
		}
		if p.MIME == engine.MimeText {
			out.Text = text
			out.Truncated = out.Truncated || trunc
		} else if p.MIME == engine.MimeHTML {
			out.HTML = text
		}
	}
	return marshalJSON(out)
}

// ReadHistoryFullText reads on-demand full UTF-8 text (bounded by maxBytes, default 2 MiB, max 16 MiB) and immediately releases its lease.
func (m *Engine) ReadHistoryFullText(historyID string, maxBytes int) (string, error) {
	if maxBytes <= 0 {
		maxBytes = 2 << 20
	}
	if maxBytes > 16<<20 {
		maxBytes = 16 << 20
	}
	return m.ReadHistoryTextPreview(historyID, maxBytes)
}

// ExportHistoryPartToFile exports the first matching part of a history record (by MIME prefix or exact MIME) to destPath and releases its lease.
func (m *Engine) ExportHistoryPartToFile(historyID string, mimePrefix string, destPath string) (string, error) {
	h, err := m.e.RetainHistoryContent(context.Background(), engine.HistoryID(historyID))
	if err != nil {
		return "", wrapErr(err)
	}
	defer func() { _ = m.e.ReleaseContent(context.Background(), h.LeaseID) }()

	var chosen *engine.PartDescriptor
	for i := range h.Content.Parts {
		p := &h.Content.Parts[i]
		if mimePrefix == "" || strings.HasPrefix(p.MIME, mimePrefix) || (mimePrefix == "file" && p.Kind == engine.PartFile) {
			chosen = p
			break
		}
	}
	if chosen == nil {
		return "", wrapErr(errors.New("matching part not found"))
	}
	rd, rerr := m.e.ReadContent(context.Background(), h.LeaseID, chosen.ID)
	if rerr != nil {
		return "", wrapErr(rerr)
	}
	cs := &ContentStream{reader: rd}
	written, werr := cs.WriteToFile(destPath)
	if werr != nil {
		return "", werr
	}
	return marshalJSON(map[string]any{
		"part":          chosen,
		"bytes_written": written,
		"path":          destPath,
	})
}

// ReleaseContent releases a LeaseID idempotently.
func (m *Engine) ReleaseContent(leaseID string) error {
	return wrapErr(m.e.ReleaseContent(context.Background(), engine.LeaseID(leaseID)))
}

// ---- Submission & History ----

type commitReqDTO struct {
	CancelOnContext bool              `json:"cancel_on_context"`
	RequestID       engine.RequestID  `json:"request_id"`
	LeaseID         engine.LeaseID    `json:"lease_id"`
	BackendIDs      []engine.PluginID `json:"backend_ids,omitempty"`
	DeviceIDs       []engine.DeviceID `json:"device_ids,omitempty"`
	LocalOnly       bool              `json:"local_only,omitempty"`
}

// CommitContent commits leased content through the pipeline and optionally syncs it.
func (m *Engine) CommitContent(token *CancelToken, reqJSON string) (string, error) {
	var dto commitReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid commit request json"))
	}
	ctx, cancel := withToken(token)
	defer cancel()
	var endTrace func(engine.DiagnosticResult, engine.Code)
	if _, existsErr := m.e.GetSubmission(ctx, dto.RequestID); existsErr != nil {
		ctx, endTrace = m.diagnostics.controller.StartSubmission(ctx)
	}
	res, err := m.e.CommitContent(ctx, engine.CommitRequest{
		CancelOnContext: dto.CancelOnContext,
		RequestID:       dto.RequestID,
		LeaseID:         dto.LeaseID,
		BackendIDs:      dto.BackendIDs,
		DeviceIDs:       dto.DeviceIDs,
		LocalOnly:       dto.LocalOnly,
	})
	if endTrace != nil {
		endTrace(traceOutcome(err))
	}
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(res)
}

type resendHistoryReqDTO struct {
	CancelOnContext bool              `json:"cancel_on_context"`
	RequestID       engine.RequestID  `json:"request_id"`
	HistoryID       engine.HistoryID  `json:"history_id"`
	BackendIDs      []engine.PluginID `json:"backend_ids,omitempty"`
	DeviceIDs       []engine.DeviceID `json:"device_ids,omitempty"`
}

// ResendHistoryItem sends the stored record to explicitly selected devices.
func (m *Engine) ResendHistoryItem(token *CancelToken, reqJSON string) (string, error) {
	var dto resendHistoryReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid resend history request json"))
	}
	ctx, cancel := withToken(token)
	defer cancel()
	var endTrace func(engine.DiagnosticResult, engine.Code)
	if _, existsErr := m.e.GetSubmission(ctx, dto.RequestID); existsErr != nil {
		ctx, endTrace = m.diagnostics.controller.StartSubmission(ctx)
	}
	res, err := m.e.SendHistory(ctx, engine.SendHistoryRequest{
		CancelOnContext: dto.CancelOnContext,
		RequestID:       dto.RequestID,
		HistoryID:       dto.HistoryID,
		BackendIDs:      dto.BackendIDs,
		DeviceIDs:       dto.DeviceIDs,
	})
	if endTrace != nil {
		endTrace(traceOutcome(err))
	}
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(res)
}

// GetSubmission returns the retained submission result for a RequestID.
func (m *Engine) GetSubmission(requestID string) (string, error) {
	res, err := m.e.GetSubmission(context.Background(), engine.RequestID(requestID))
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(res)
}

type historyFilterDTO struct {
	Kinds           []engine.HistoryKind       `json:"kinds,omitempty"`
	Tags            []string                   `json:"tags,omitempty"`
	OriginKinds     []engine.HistoryOriginKind `json:"origin_kinds,omitempty"`
	DeviceID        engine.DeviceID            `json:"device_id,omitempty"`
	CreatedFromMs   int64                      `json:"created_from_ms,omitempty"`
	CreatedBeforeMs int64                      `json:"created_before_ms,omitempty"`
}

type listHistoryReqDTO struct {
	Limit  int              `json:"limit,omitempty"`
	Cursor string           `json:"cursor,omitempty"`
	Filter historyFilterDTO `json:"filter,omitempty"`
}

// ListHistory queries persisted local history with paging and filters.
func (m *Engine) ListHistory(reqJSON string) (string, error) {
	var dto listHistoryReqDTO
	if strings.TrimSpace(reqJSON) != "" {
		if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
			return "", wrapErr(errors.New("invalid list history json"))
		}
	}
	page, err := m.e.ListHistory(context.Background(), engine.ListHistoryRequest{
		Limit:  dto.Limit,
		Cursor: dto.Cursor,
		Filter: engine.HistoryFilter{
			Kinds:           dto.Filter.Kinds,
			Tags:            dto.Filter.Tags,
			OriginKinds:     dto.Filter.OriginKinds,
			DeviceID:        dto.Filter.DeviceID,
			CreatedFromMs:   dto.Filter.CreatedFromMs,
			CreatedBeforeMs: dto.Filter.CreatedBeforeMs,
		},
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(page)
}

// GetHistoryItem fetches a single history item's metadata.
func (m *Engine) GetHistoryItem(historyID string) (string, error) {
	item, err := m.e.GetItem(context.Background(), engine.HistoryID(historyID))
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(item)
}

type updateItemReqDTO struct {
	ID               engine.HistoryID `json:"id"`
	ExpectedRevision json.RawMessage  `json:"expected_revision"`
	Tags             *[]string        `json:"tags,omitempty"`
	ExpiresAtMs      *int64           `json:"expires_at_ms,omitempty"`
}

// UpdateHistoryItem updates tags or expiry with optimistic revision checking.
func (m *Engine) UpdateHistoryItem(reqJSON string) (string, error) {
	var dto updateItemReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid update item json"))
	}
	rev, err := parseUint64Flexible(dto.ExpectedRevision)
	if err != nil {
		return "", wrapErr(errors.New("invalid expected_revision"))
	}
	item, err := m.e.UpdateItem(context.Background(), engine.UpdateItemRequest{
		ID:               dto.ID,
		ExpectedRevision: rev,
		Tags:             dto.Tags,
		ExpiresAtMs:      dto.ExpiresAtMs,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(item)
}

// DeleteHistoryItem removes a history record given its decimal string revision.
func (m *Engine) DeleteHistoryItem(historyID string, expectedRevision string) error {
	rev, err := strconv.ParseUint(strings.TrimSpace(expectedRevision), 10, 64)
	if err != nil {
		return wrapErr(errors.New("invalid expected_revision"))
	}
	return wrapErr(m.e.DeleteItem(context.Background(), engine.DeleteItemRequest{
		ID:               engine.HistoryID(historyID),
		ExpectedRevision: rev,
	}))
}

// ListHistoryTags lists all defined and attached history tags.
func (m *Engine) ListHistoryTags() (string, error) {
	tags, err := m.e.ListHistoryTags(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	if tags == nil {
		tags = []string{}
	}
	return marshalJSON(tags)
}

// SearchHistory executes a bounded history search query.
func (m *Engine) SearchHistory(token *CancelToken, reqJSON string) (string, error) {
	var req engine.HistorySearchRequest
	if err := json.Unmarshal([]byte(reqJSON), &req); err != nil {
		return "", wrapErr(errors.New("invalid search request json"))
	}
	ctx, cancel := withToken(token)
	defer cancel()
	page, err := m.e.SearchHistory(ctx, req)
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(page)
}

// CountHistorySearches computes prospective counts for typed search queries.
func (m *Engine) CountHistorySearches(token *CancelToken, reqJSON string) (string, error) {
	var req engine.HistorySearchCountsRequest
	if err := json.Unmarshal([]byte(reqJSON), &req); err != nil {
		return "", wrapErr(errors.New("invalid count request json"))
	}
	ctx, cancel := withToken(token)
	defer cancel()
	counts, err := m.e.CountHistorySearches(ctx, req)
	if err != nil {
		return "", wrapErr(err)
	}
	out := struct {
		Counts          []string `json:"counts"`
		HistoryRevision string   `json:"history_revision"`
		EvaluatedAtMs   int64    `json:"evaluated_at_ms"`
	}{
		Counts:          make([]string, len(counts.Counts)),
		HistoryRevision: strconv.FormatUint(counts.HistoryRevision, 10),
		EvaluatedAtMs:   counts.EvaluatedAtMs,
	}
	for i, c := range counts.Counts {
		out.Counts[i] = strconv.FormatUint(c, 10)
	}
	return marshalJSON(out)
}

// ListActions returns available content transformation actions.
func (m *Engine) ListActions() (string, error) {
	acts, err := m.e.ListActions(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	if acts == nil {
		acts = []engine.ActionInfo{}
	}
	return marshalJSON(acts)
}

type runActionReqDTO struct {
	CancelOnContext bool             `json:"cancel_on_context"`
	RequestID       engine.RequestID `json:"request_id"`
	Key             string           `json:"key"`
	HistoryID       engine.HistoryID `json:"history_id"`
}

// RunAction executes an action on a stored history item.
func (m *Engine) RunAction(token *CancelToken, reqJSON string) (string, error) {
	var dto runActionReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid run action json"))
	}
	ctx, cancel := withToken(token)
	defer cancel()
	res, err := m.e.RunAction(ctx, engine.RunActionRequest{
		CancelOnContext: dto.CancelOnContext,
		RequestID:       dto.RequestID,
		Key:             dto.Key,
		HistoryID:       dto.HistoryID,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(res)
}

// ---- Clipboard Observe / Apply Anti-Loopback ----

type observeReqDTO struct {
	Epoch        string             `json:"epoch"`
	Sequence     json.RawMessage    `json:"sequence"`
	Mode         engine.ObserveMode `json:"mode"`
	LeaseID      engine.LeaseID     `json:"lease_id,omitempty"`
	AppliedToken engine.ApplyToken  `json:"applied_token,omitempty"`
}

// ObserveClipboard reports a baseline or change observation from the host clipboard.
func (m *Engine) ObserveClipboard(reqJSON string) (string, error) {
	var dto observeReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid observe json"))
	}
	seq, err := parseUint64Flexible(dto.Sequence)
	if err != nil {
		return "", wrapErr(errors.New("invalid sequence"))
	}
	res, err := m.e.ObserveClipboard(context.Background(), engine.Observation{
		Epoch:        dto.Epoch,
		Sequence:     seq,
		Mode:         dto.Mode,
		LeaseID:      dto.LeaseID,
		AppliedToken: dto.AppliedToken,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(map[string]string{
		"disposition":  string(res.Disposition),
		"operation_id": string(res.OperationID),
	})
}

type applyReqDTO struct {
	ReceiptID        engine.ReceiptID `json:"receipt_id"`
	ExpectedEpoch    string           `json:"expected_epoch"`
	ExpectedSequence json.RawMessage  `json:"expected_sequence"`
}

func marshalApplyTicket(t engine.ApplyTicket) (string, error) {
	return marshalJSON(map[string]string{
		"token":      string(t.Token),
		"content_id": string(t.ContentID),
		"lease_id":   string(t.LeaseID),
	})
}

// BeginApply starts writing a received content entry to the system clipboard.
func (m *Engine) BeginApply(reqJSON string) (string, error) {
	var dto applyReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid apply request json"))
	}
	seq, err := parseUint64Flexible(dto.ExpectedSequence)
	if err != nil {
		return "", wrapErr(errors.New("invalid expected_sequence"))
	}
	t, err := m.e.BeginApply(context.Background(), engine.ApplyRequest{
		ReceiptID:        dto.ReceiptID,
		ExpectedEpoch:    dto.ExpectedEpoch,
		ExpectedSequence: seq,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalApplyTicket(t)
}

// BeginApplyHistory starts writing a persisted history item to the system clipboard.
func (m *Engine) BeginApplyHistory(historyID string, expectedEpoch string, expectedSequence string, userRestore bool) (string, error) {
	var seq uint64
	if strings.TrimSpace(expectedSequence) != "" {
		var err error
		seq, err = strconv.ParseUint(strings.TrimSpace(expectedSequence), 10, 64)
		if err != nil {
			return "", wrapErr(errors.New("invalid expected_sequence"))
		}
	}
	t, err := m.e.BeginApplyHistory(context.Background(), engine.ApplyHistoryRequest{
		UserRestore:      userRestore,
		HistoryID:        engine.HistoryID(historyID),
		ExpectedEpoch:    expectedEpoch,
		ExpectedSequence: seq,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalApplyTicket(t)
}

type applyResultReqDTO struct {
	Token           engine.ApplyToken   `json:"token"`
	Outcome         engine.ApplyOutcome `json:"outcome"`
	BaselineLeaseID engine.LeaseID      `json:"baseline_lease_id,omitempty"`
	Epoch           string              `json:"epoch,omitempty"`
	Sequence        json.RawMessage     `json:"sequence,omitempty"`
	ErrorCode       string              `json:"error_code,omitempty"`
}

// ReportApplyResult closes an active apply ticket and updates the observation baseline on success.
func (m *Engine) ReportApplyResult(reqJSON string) error {
	var dto applyResultReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return wrapErr(errors.New("invalid apply result json"))
	}
	seq, err := parseUint64Flexible(dto.Sequence)
	if err != nil {
		return wrapErr(errors.New("invalid sequence"))
	}
	return wrapErr(m.e.ReportApplyResult(context.Background(), engine.ApplyResult{
		Token:           dto.Token,
		Outcome:         dto.Outcome,
		BaselineLeaseID: dto.BaselineLeaseID,
		Epoch:           dto.Epoch,
		Sequence:        seq,
		ErrorCode:       dto.ErrorCode,
	}))
}

// ListReceived returns transient received entries awaiting application.
func (m *Engine) ListReceived() (string, error) {
	rs, err := m.e.ListReceived(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	if rs == nil {
		rs = []engine.ReceivedContent{}
	}
	return marshalJSON(rs)
}

// DismissReceived removes an entry from the transient received list.
func (m *Engine) DismissReceived(receiptID string) error {
	return wrapErr(m.e.DismissReceived(context.Background(), engine.ReceiptID(receiptID)))
}

// ---- Devices & Pairing ----

// CreatePairingCode generates a temporary 6-digit pairing code.
func (m *Engine) CreatePairingCode(token *CancelToken) (string, error) {
	ctx, cancel := withToken(token)
	defer cancel()
	c, err := m.e.CreatePairingCode(ctx)
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(c)
}

// GetPairingCode returns the active pairing code if one exists and has not expired.
func (m *Engine) GetPairingCode() (string, error) {
	c, exists, err := m.e.GetPairingCode(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(map[string]any{
		"exists":        exists,
		"code":          c.Code,
		"expires_at_ms": c.ExpiresAtMs,
	})
}

// CancelPairingCode invalidates any active pairing code.
func (m *Engine) CancelPairingCode() error {
	return wrapErr(m.e.CancelPairingCode(context.Background()))
}

// ExportPairingTicket returns a sensitive full pairing ticket for offline/QR pairing.
func (m *Engine) ExportPairingTicket(token *CancelToken) (string, error) {
	ctx, cancel := withToken(token)
	defer cancel()
	t, err := m.e.ExportPairingTicket(ctx)
	if err != nil {
		return "", wrapErr(err)
	}
	result := struct {
		engine.SensitiveTicket
		QRPNG string `json:"qr_png,omitempty"`
	}{SensitiveTicket: t}
	if png, err := qrcode.Encode(t.Ticket, qrcode.Low, 512); err == nil {
		result.QRPNG = base64.StdEncoding.EncodeToString(png)
	}
	return marshalJSON(result)
}

type joinReqDTO struct {
	BackendID engine.PluginID  `json:"backend_id,omitempty"`
	RequestID engine.RequestID `json:"request_id"`
	Code      string           `json:"code"`
}

// JoinByCode pairs with another device using a 6-digit code and returns its OperationID.
func (m *Engine) JoinByCode(token *CancelToken, reqJSON string) (string, error) {
	var dto joinReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid join request json"))
	}
	ctx, cancel := withToken(token)
	defer cancel()
	opID, err := m.e.JoinByCode(ctx, engine.JoinRequest{
		BackendID: dto.BackendID,
		RequestID: dto.RequestID,
		Code:      dto.Code,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return string(opID), nil
}

type joinTicketReqDTO struct {
	BackendID engine.PluginID  `json:"backend_id,omitempty"`
	RequestID engine.RequestID `json:"request_id"`
	Ticket    string           `json:"ticket"`
}

// JoinByTicket pairs with another device using an exported pairing ticket and returns its OperationID.
func (m *Engine) JoinByTicket(token *CancelToken, reqJSON string) (string, error) {
	var dto joinTicketReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid join ticket json"))
	}
	ctx, cancel := withToken(token)
	defer cancel()
	opID, err := m.e.JoinByTicket(ctx, engine.JoinTicketRequest{
		BackendID: dto.BackendID,
		RequestID: dto.RequestID,
		Ticket:    dto.Ticket,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return string(opID), nil
}

// GetOperation returns an operation's current state JSON.
func (m *Engine) GetOperation(opID string) (string, error) {
	op, err := m.e.GetOperation(context.Background(), engine.OperationID(opID))
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(op)
}

// ListDevices returns all non-revoked devices with their bindings.
func (m *Engine) ListDevices() (string, error) {
	ds, err := m.e.ListDevices(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	if ds == nil {
		ds = []engine.Device{}
	}
	return marshalJSON(ds)
}

type bindingReqDTO struct {
	ID               string `json:"id"`
	ExpectedRevision int64  `json:"expected_revision"`
}

type approveDeviceReqDTO struct {
	ID                  string          `json:"id"`
	ExpectedRevision    int64           `json:"expected_revision"`
	ExpectedFingerprint string          `json:"expected_fingerprint"`
	Bindings            []bindingReqDTO `json:"bindings"`
}

// ApproveDevice trusts a pending device and authorizes the reviewed bindings.
func (m *Engine) ApproveDevice(reqJSON string) (string, error) {
	var dto approveDeviceReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid approve device json"))
	}
	bindings := make([]engine.BindingRequest, len(dto.Bindings))
	for i, b := range dto.Bindings {
		bindings[i] = engine.BindingRequest{ID: b.ID, ExpectedRevision: b.ExpectedRevision}
	}
	d, err := m.e.ApproveDevice(context.Background(), engine.ApproveDeviceRequest{
		ID:                  dto.ID,
		ExpectedRevision:    dto.ExpectedRevision,
		ExpectedFingerprint: dto.ExpectedFingerprint,
		Bindings:            bindings,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(d)
}

type renameDeviceReqDTO struct {
	ID               string `json:"id"`
	ExpectedRevision int64  `json:"expected_revision"`
	Name             string `json:"name"`
}

// RenameDevice updates the user-assigned name for a device.
func (m *Engine) RenameDevice(reqJSON string) (string, error) {
	var dto renameDeviceReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid rename device json"))
	}
	d, err := m.e.RenameDevice(context.Background(), engine.RenameDeviceRequest{
		ID:               dto.ID,
		ExpectedRevision: dto.ExpectedRevision,
		Name:             dto.Name,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(d)
}

type setBindingSyncReqDTO struct {
	ID               string `json:"id"`
	ExpectedRevision int64  `json:"expected_revision"`
	Enabled          bool   `json:"enabled"`
}

// SetBindingSync pauses or resumes content sync on a single device binding.
func (m *Engine) SetBindingSync(reqJSON string) (string, error) {
	var dto setBindingSyncReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid set binding sync json"))
	}
	b, err := m.e.SetBindingSync(context.Background(), engine.SetBindingSyncRequest{
		ID:               dto.ID,
		ExpectedRevision: dto.ExpectedRevision,
		Enabled:          dto.Enabled,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(b)
}

// UnpairDevice revokes trust in a device and removes all paths to it.
func (m *Engine) UnpairDevice(deviceID string, expectedRevision int64) error {
	return wrapErr(m.e.UnpairDevice(context.Background(), engine.UnpairDeviceRequest{
		ID:               deviceID,
		ExpectedRevision: expectedRevision,
	}))
}

// ListCandidates returns device candidates introduced by trusted peers.
func (m *Engine) ListCandidates() (string, error) {
	cs, err := m.e.ListCandidates(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	if cs == nil {
		cs = []engine.Candidate{}
	}
	return marshalJSON(cs)
}

type pairCandidateReqDTO struct {
	RequestID engine.RequestID `json:"request_id"`
	DeviceID  string           `json:"device_id"`
	BackendID engine.PluginID  `json:"backend_id"`
}

// PairCandidate initiates pairing with a suggested candidate device.
func (m *Engine) PairCandidate(reqJSON string) (string, error) {
	var dto pairCandidateReqDTO
	if err := json.Unmarshal([]byte(reqJSON), &dto); err != nil {
		return "", wrapErr(errors.New("invalid pair candidate json"))
	}
	opID, err := m.e.PairCandidate(context.Background(), engine.PairCandidateRequest{
		RequestID: dto.RequestID,
		DeviceID:  dto.DeviceID,
		BackendID: dto.BackendID,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return string(opID), nil
}

// IgnoreCandidate hides or restores an introduced candidate device.
func (m *Engine) IgnoreCandidate(deviceID string, ignored bool) error {
	return wrapErr(m.e.IgnoreCandidate(context.Background(), deviceID, ignored))
}
