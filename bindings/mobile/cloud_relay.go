package mobile

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/fsnotify/fsnotify"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/cloudauth"
	"github.com/uniclipboard/unic/internal/relayclient"
	"github.com/uniclipboard/unic/internal/relaycrypto"
)

const (
	mobileCloudSessionDebounce  = 100 * time.Millisecond
	mobileCloudRotateMinDelay   = 100 * time.Millisecond
	mobileCloudRotateMinBackoff = 500 * time.Millisecond
	mobileCloudRotateMaxBackoff = 5 * time.Minute
	mobileCloudCredentialSkew   = 60 * time.Second
)

type mobileCloudRelay struct {
	owner   *Engine
	dir     string
	httpCli *http.Client
	mgr     *cloudauth.Manager
	nowFn   func() time.Time

	opMu sync.Mutex

	cfgMu           sync.RWMutex
	syncEnabled     bool
	relayEnabled    bool
	relayURL        string
	manualKey       string
	manualKeyTarget string
	manualKeyBound  bool
	deviceNameV     string

	loopMu     sync.Mutex
	loopCancel context.CancelFunc
	loopWG     sync.WaitGroup
	wake       chan struct{}

	stateMu         sync.Mutex
	engineRelayHeld bool
	engineRelayURL  string
}

func newMobileCloudRelay(owner *Engine, dir string, httpCli *http.Client, nowFn func() time.Time) *mobileCloudRelay {
	if nowFn == nil {
		nowFn = time.Now
	}
	c := &mobileCloudRelay{
		owner:   owner,
		dir:     strings.TrimSpace(dir),
		httpCli: httpCli,
		nowFn:   nowFn,
		wake:    make(chan struct{}, 1),
	}
	c.mgr = cloudauth.NewManager(c.dir, cloudauth.ManagerOptions{
		HTTPClient: httpCli,
		Now:        nowFn,
		DeviceName: func() string {
			return c.deviceName()
		},
		OnRevoked: func() {
			c.handleRevoked()
		},
	})
	return c
}

func (c *mobileCloudRelay) deviceName() string {
	c.cfgMu.RLock()
	name := strings.TrimSpace(c.deviceNameV)
	c.cfgMu.RUnlock()
	return name
}

func (c *mobileCloudRelay) setStoredConfig(s engine.Settings, manualKey, manualKeyTarget string, manualKeyBound bool) {
	urlStr := strings.TrimSpace(s.Sync.Relay.URL)
	keyStr := strings.TrimSpace(manualKey)
	normTarget, ok := canonicalRelayTarget(urlStr)
	if keyStr == "" || !manualKeyBound || !ok || normTarget != manualKeyTarget {
		keyStr = ""
		manualKeyTarget = ""
		manualKeyBound = false
	}
	c.cfgMu.Lock()
	c.syncEnabled = s.Sync.Enabled
	c.relayEnabled = s.Sync.Relay.Enabled
	c.relayURL = urlStr
	c.manualKey = keyStr
	c.manualKeyTarget = manualKeyTarget
	c.manualKeyBound = manualKeyBound
	c.deviceNameV = strings.TrimSpace(s.DeviceName)
	c.cfgMu.Unlock()
}

func (c *mobileCloudRelay) storedConfig() (syncEnabled, relayEnabled bool, relayURL, manualKey string) {
	c.cfgMu.RLock()
	syncEnabled = c.syncEnabled
	relayEnabled = c.relayEnabled
	relayURL = c.relayURL
	if c.manualKey != "" && c.manualKeyBound {
		if normTarget, ok := canonicalRelayTarget(c.relayURL); ok && normTarget == c.manualKeyTarget {
			manualKey = c.manualKey
		}
	}
	c.cfgMu.RUnlock()
	return syncEnabled, relayEnabled, relayURL, manualKey
}

func (c *mobileCloudRelay) isOfficialURL(rawURL string) bool {
	if strings.TrimSpace(rawURL) != "" {
		return false
	}
	envURL := strings.TrimSpace(os.Getenv("UNIC_RELAY_URL"))
	if envURL == "" {
		return true
	}
	normEnv, err := relaycrypto.NormalizeRelayOrigin(envURL)
	if err != nil {
		return false
	}
	if normEnv == relayclient.DefaultURL {
		return true
	}
	if strings.TrimSpace(os.Getenv("UNIC_CLOUD_URL")) != "" {
		sess, ok := c.mgr.Current()
		if !ok || sess.DeviceCredential == nil || strings.TrimSpace(sess.DeviceCredential.RelayURL) == "" {
			return true
		}
		return normEnv == mobileOfficialRelayOrigin(sess)
	}
	return false
}

func mobileOfficialRelayOrigin(sess cloudauth.Session) string {
	if sess.DeviceCredential != nil && strings.TrimSpace(sess.DeviceCredential.RelayURL) != "" {
		norm, err := cloudauth.NormalizeAdvertisedRelayURL(sess.DeviceCredential.RelayURL)
		if err != nil {
			return ""
		}
		if norm != "" {
			return norm
		}
	}
	return relayclient.DefaultURL
}

func allowedMobileStaticOrigin(relayURL string) string {
	if u := strings.TrimSpace(relayURL); u != "" {
		if norm, err := relaycrypto.NormalizeRelayOrigin(u); err == nil {
			return norm
		}
		return ""
	}
	if envURL := strings.TrimSpace(os.Getenv("UNIC_RELAY_URL")); envURL != "" {
		if norm, err := relaycrypto.NormalizeRelayOrigin(envURL); err == nil {
			return norm
		}
		return ""
	}
	return relayclient.DefaultURL
}

func effectiveMobileManualKey(manualKey string) string {
	if k := strings.TrimSpace(manualKey); k != "" {
		return k
	}
	return strings.TrimSpace(os.Getenv("UNIC_RELAY_ACCESS_KEY"))
}

// AccessKey implements engine.RelayAuthProvider.
func (c *mobileCloudRelay) AccessKey(origin string) (string, bool) {
	if c == nil || c.mgr == nil {
		return "", false
	}
	syncEnabled, relayEnabled, rawURL, manualKey := c.storedConfig()
	if !syncEnabled || !relayEnabled {
		return "", false
	}
	normOrigin, err := relaycrypto.NormalizeRelayOrigin(origin)
	if err != nil {
		return "", false
	}
	if !c.isOfficialURL(rawURL) {
		if normOrigin != allowedMobileStaticOrigin(rawURL) {
			return "", false
		}
		return effectiveMobileManualKey(manualKey), true
	}
	sess, ok := c.mgr.Current()
	if ok && sess.HasActiveCredential(c.nowFn()) {
		expected := mobileOfficialRelayOrigin(sess)
		if expected != "" && normOrigin == expected {
			return strings.TrimSpace(sess.DeviceCredential.Credential), true
		}
		return "", false
	}
	if k := effectiveMobileManualKey(manualKey); k != "" && normOrigin == allowedMobileStaticOrigin(rawURL) {
		return k, true
	}
	return "", false
}

// OnUnauthorized implements engine.RelayAuthProvider.
func (c *mobileCloudRelay) OnUnauthorized(ctx context.Context, origin, rejectedKey string) bool {
	if c == nil || c.mgr == nil {
		return false
	}
	c.opMu.Lock()
	syncEnabled, relayEnabled, rawURL, _ := c.storedConfig()
	if !syncEnabled || !relayEnabled || !c.isOfficialURL(rawURL) {
		c.opMu.Unlock()
		return false
	}
	sess, ok := c.mgr.Current()
	if !ok || sess.EffectiveState() != cloudauth.SessionStateActive {
		c.opMu.Unlock()
		return false
	}
	normOrigin, err := relaycrypto.NormalizeRelayOrigin(origin)
	if err != nil {
		c.opMu.Unlock()
		return false
	}
	expected := mobileOfficialRelayOrigin(sess)
	if sess.DeviceCredential != nil && strings.TrimSpace(sess.DeviceCredential.RelayURL) != "" {
		if normOrigin != expected {
			c.opMu.Unlock()
			return false
		}
	} else if normOrigin != relayclient.DefaultURL && strings.TrimSpace(os.Getenv("UNIC_CLOUD_URL")) == "" {
		c.opMu.Unlock()
		return false
	}

	rotated, updated, _ := c.mgr.HandleRelayUnauthorized(ctx, rejectedKey)
	c.opMu.Unlock()

	if updated.EffectiveState() == cloudauth.SessionStateReauthRequired && c.owner != nil {
		// Never call reconcileCloudRelayState synchronously here: OnUnauthorized can be
		// invoked from engine.relay.runLoop (inside relay.wg), and UpdateSettings stops
		// the relay loop with relay.wg.Wait(), which would self-deadlock.
		c.loopMu.Lock()
		loopRunning := c.loopCancel != nil
		c.loopMu.Unlock()
		if loopRunning {
			c.notify()
		} else {
			go c.owner.reconcileCloudRelayState(context.Background())
		}
	}
	if !rotated {
		if !updated.HasActiveCredential(c.nowFn()) {
			c.notify()
		}
		return false
	}
	if updated.DeviceCredential == nil || mobileOfficialRelayOrigin(updated) != normOrigin {
		c.notify()
		return false
	}
	c.notify()
	return true
}

func (c *mobileCloudRelay) shouldDisableOfficialRelay(s engine.Settings, manualKey string) bool {
	if !c.isOfficialURL(s.Sync.Relay.URL) || !s.Sync.Relay.Enabled || effectiveMobileManualKey(manualKey) != "" {
		return false
	}
	sess, ok := c.mgr.Current()
	return !ok || sess.EffectiveState() != cloudauth.SessionStateActive
}

func (c *mobileCloudRelay) shouldHoldEngineOfficialRelay(s engine.Settings, manualKey string) bool {
	if !c.isOfficialURL(s.Sync.Relay.URL) || !s.Sync.Relay.Enabled || effectiveMobileManualKey(manualKey) != "" {
		return false
	}
	sess, ok := c.mgr.Current()
	return !ok || !sess.HasActiveCredential(c.nowFn())
}

func (c *mobileCloudRelay) applyRelayGuard(s engine.Settings, manualKey string) engine.Settings {
	out := s
	out.Sync.Relay.AccessKey = strings.TrimSpace(manualKey)
	held := c.shouldHoldEngineOfficialRelay(s, manualKey)
	if held {
		out.Sync.Relay.Enabled = false
	} else if c.isOfficialURL(s.Sync.Relay.URL) && strings.TrimSpace(out.Sync.Relay.URL) == "" {
		if sess, ok := c.mgr.Current(); ok && sess.HasActiveCredential(c.nowFn()) {
			if origin := mobileOfficialRelayOrigin(sess); origin != "" && origin != relayclient.DefaultURL {
				out.Sync.Relay.URL = origin
			}
		}
	}
	c.setEngineRelayState(held, out.Sync.Relay.URL)
	return out
}

func (c *mobileCloudRelay) setEngineRelayState(held bool, targetURL string) {
	c.stateMu.Lock()
	c.engineRelayHeld = held
	c.engineRelayURL = strings.TrimSpace(targetURL)
	c.stateMu.Unlock()
}

func (c *mobileCloudRelay) engineRelayState() (bool, string) {
	c.stateMu.Lock()
	defer c.stateMu.Unlock()
	return c.engineRelayHeld, c.engineRelayURL
}

func (c *mobileCloudRelay) notify() {
	if c == nil {
		return
	}
	select {
	case c.wake <- struct{}{}:
	default:
	}
}

func (c *mobileCloudRelay) handleRevoked() {
	if c == nil {
		return
	}
	c.notify()
}

func (c *mobileCloudRelay) start() {
	if c == nil || c.mgr == nil {
		return
	}
	c.loopMu.Lock()
	if c.loopCancel != nil {
		c.loopMu.Unlock()
		c.notify()
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	c.loopCancel = cancel
	c.loopWG.Add(2)
	c.loopMu.Unlock()

	go func() {
		defer c.loopWG.Done()
		c.runRotationLoop(ctx)
	}()
	go func() {
		defer c.loopWG.Done()
		c.watchSessionFile(ctx)
	}()
}

func (c *mobileCloudRelay) stop() {
	if c == nil {
		return
	}
	c.loopMu.Lock()
	cancel := c.loopCancel
	c.loopCancel = nil
	c.loopMu.Unlock()
	if cancel != nil {
		cancel()
	}
	c.loopWG.Wait()
}

func (c *mobileCloudRelay) runRotationLoop(ctx context.Context) {
	backoff := time.Duration(0)
	var nextRetryAt time.Time
	_, _, initURL, _ := c.storedConfig()
	lastOfficialMode := c.isOfficialURL(initURL)
	var timer *time.Timer
	stopTimer := func() {
		if timer == nil {
			return
		}
		if !timer.Stop() {
			select {
			case <-timer.C:
			default:
			}
		}
	}
	defer stopTimer()

	for {
		var needPush bool
		var needDisable bool
		var wantHold bool
		var sess cloudauth.Session
		var ok bool
		var err error

		c.opMu.Lock()
		now := c.nowFn()
		beforeSess, beforeOK := c.mgr.Current()
		sess, ok, err = c.mgr.Reload()
		syncEnabled, relayEnabled, rawURL, manualKey := c.storedConfig()
		if err == nil {
			official := c.isOfficialURL(rawURL)
			if official != lastOfficialMode || mobileCredChanged(beforeSess, beforeOK, sess, ok) {
				backoff = 0
				nextRetryAt = time.Time{}
				lastOfficialMode = official
				if mobileCredChanged(beforeSess, beforeOK, sess, ok) {
					needPush = true
				}
			}
			if !official {
				backoff = 0
				nextRetryAt = time.Time{}
				if ok && sess.EffectiveState() == cloudauth.SessionStateActive && sess.DeviceCredential != nil {
					revCtx, cancel := context.WithTimeout(ctx, 3*time.Second)
					_ = c.mgr.ClearDeviceCredential(revCtx)
					cancel()
					sess, ok = c.mgr.Current()
					needPush = true
				}
			} else if ok {
				switch sess.EffectiveState() {
				case cloudauth.SessionStateReauthRequired:
					backoff = 0
					nextRetryAt = time.Time{}
					if relayEnabled && effectiveMobileManualKey(manualKey) == "" {
						needDisable = true
					}
				case cloudauth.SessionStateActive:
					if syncEnabled && relayEnabled && sess.NeedsRotation(now) {
						if nextRetryAt.IsZero() || !now.Before(nextRetryAt) {
							rctx, cancel := context.WithTimeout(ctx, 25*time.Second)
							rotated, updated, rerr := c.mgr.RotateIfDue(rctx)
							cancel()
							sess = updated
							ok = updated.EffectiveState() != ""
							if rerr != nil {
								if updated.EffectiveState() == cloudauth.SessionStateReauthRequired {
									backoff = 0
									nextRetryAt = time.Time{}
									if relayEnabled && effectiveMobileManualKey(manualKey) == "" {
										needDisable = true
									}
								} else {
									if backoff == 0 {
										backoff = mobileCloudRotateMinBackoff
									} else if backoff < mobileCloudRotateMaxBackoff {
										backoff *= 2
										if backoff > mobileCloudRotateMaxBackoff {
											backoff = mobileCloudRotateMaxBackoff
										}
									}
									nextRetryAt = c.nowFn().Add(backoff)
								}
							} else {
								backoff = 0
								nextRetryAt = time.Time{}
								if rotated {
									needPush = true
								}
							}
						}
					} else {
						backoff = 0
						nextRetryAt = time.Time{}
					}
				}
			} else {
				backoff = 0
				nextRetryAt = time.Time{}
				if relayEnabled && effectiveMobileManualKey(manualKey) == "" {
					needDisable = true
				}
			}

			// Re-read live storedConfig after potentially slow Cloud network calls so a concurrent
			// UpdateSettings transition off official mode is never overwritten by stale local state.
			syncEnabled, relayEnabled, rawURL, manualKey = c.storedConfig()
			curOfficial := c.isOfficialURL(rawURL)
			if curOfficial != official {
				needPush = true
				if !curOfficial {
					c.notify()
				}
			}

			curSettings := engine.Settings{
				Sync: engine.SyncSettings{
					Enabled: syncEnabled,
					Relay: engine.RelaySyncSettings{
						Enabled: relayEnabled,
						URL:     rawURL,
					},
				},
			}
			wantHold = c.shouldHoldEngineOfficialRelay(curSettings, manualKey)
			wantURL := c.desiredEngineURL(curSettings, manualKey, sess, ok, now)
			prevHold, prevURL := c.engineRelayState()
			if wantHold != prevHold || wantURL != prevURL {
				needPush = true
			}
		}
		c.opMu.Unlock()

		if (needDisable || needPush) && c.owner != nil {
			c.owner.reconcileCloudRelayState(ctx)
			if !wantHold && c.owner.e != nil {
				c.owner.e.NotifyRelayAuthChanged()
			}
		}

		syncEnabled, relayEnabled, rawURL, _ = c.storedConfig()
		var waitCh <-chan time.Time
		if d, hasWait := c.nextDelay(syncEnabled, relayEnabled, rawURL, sess, ok, backoff, nextRetryAt); hasWait {
			if timer == nil {
				timer = time.NewTimer(d)
			} else {
				stopTimer()
				timer.Reset(d)
			}
			waitCh = timer.C
		}

		select {
		case <-ctx.Done():
			return
		case <-c.wake:
		case <-waitCh:
		}
	}
}

func (c *mobileCloudRelay) desiredEngineURL(s engine.Settings, manualKey string, sess cloudauth.Session, ok bool, now time.Time) string {
	if strings.TrimSpace(s.Sync.Relay.URL) != "" {
		return strings.TrimSpace(s.Sync.Relay.URL)
	}
	if envURL := strings.TrimSpace(os.Getenv("UNIC_RELAY_URL")); envURL != "" {
		return envURL
	}
	if c.isOfficialURL(s.Sync.Relay.URL) && ok && sess.HasActiveCredential(now) && !c.shouldHoldEngineOfficialRelay(s, manualKey) {
		if origin := mobileOfficialRelayOrigin(sess); origin != "" && origin != relayclient.DefaultURL {
			return origin
		}
	}
	return ""
}

func (c *mobileCloudRelay) nextDelay(syncEnabled, relayEnabled bool, rawURL string, sess cloudauth.Session, ok bool, backoff time.Duration, nextRetryAt time.Time) (time.Duration, bool) {
	now := c.nowFn()
	nowMs := now.UnixMilli()
	var best time.Duration
	haveBest := false
	consider := func(d time.Duration) {
		if !haveBest || d < best {
			best = d
			haveBest = true
		}
	}
	official := c.isOfficialURL(rawURL)
	if backoff > 0 && !nextRetryAt.IsZero() {
		d := nextRetryAt.Sub(now)
		if d < mobileCloudRotateMinDelay {
			d = mobileCloudRotateMinDelay
		}
		consider(d)
	} else if official && syncEnabled && relayEnabled && ok && sess.EffectiveState() == cloudauth.SessionStateActive && sess.DeviceCredential != nil {
		targetMs := sess.DeviceCredential.RotateAfterMs
		if targetMs <= 0 && sess.DeviceCredential.ExpiresAtMs > 0 {
			targetMs = sess.DeviceCredential.ExpiresAtMs - mobileCloudCredentialSkew.Milliseconds()
		}
		if targetMs > 0 {
			d := time.Duration(targetMs-nowMs) * time.Millisecond
			if d < mobileCloudRotateMinDelay {
				d = mobileCloudRotateMinDelay
			}
			consider(d)
		}
	}

	held, _ := c.engineRelayState()
	if !held && official && relayEnabled && ok && sess.HasActiveCredential(now) && sess.DeviceCredential.ExpiresAtMs > 0 {
		expAt := time.UnixMilli(sess.DeviceCredential.ExpiresAtMs).Add(time.Millisecond)
		d := expAt.Sub(now)
		if d < 0 {
			d = 0
		}
		consider(d)
	}
	return best, haveBest
}

func (c *mobileCloudRelay) watchSessionFile(ctx context.Context) {
	if c == nil || c.dir == "" {
		return
	}
	w, err := fsnotify.NewWatcher()
	if err != nil {
		return
	}
	defer func() { _ = w.Close() }()
	if err := w.Add(c.dir); err != nil {
		return
	}
	target := filepath.Clean(cloudauth.SessionPath(c.dir))
	var timer *time.Timer
	var fire <-chan time.Time
	for {
		select {
		case <-ctx.Done():
			return
		case ev, ok := <-w.Events:
			if !ok {
				return
			}
			if filepath.Clean(ev.Name) != target {
				continue
			}
			if timer == nil {
				timer = time.NewTimer(mobileCloudSessionDebounce)
			} else {
				if !timer.Stop() {
					select {
					case <-timer.C:
					default:
					}
				}
				timer.Reset(mobileCloudSessionDebounce)
			}
			fire = timer.C
		case <-fire:
			fire = nil
			prev, prevOK := c.mgr.Current()
			cur, curOK, err := c.mgr.Reload()
			if err == nil {
				if mobileCredChanged(prev, prevOK, cur, curOK) && c.owner != nil {
					c.owner.reconcileCloudRelayState(ctx)
					if c.owner.e != nil {
						c.owner.e.NotifyRelayAuthChanged()
					}
				}
				c.notify()
			}
		case _, ok := <-w.Errors:
			if !ok {
				return
			}
		}
	}
}

func mobileCredChanged(a cloudauth.Session, aOK bool, b cloudauth.Session, bOK bool) bool {
	if aOK != bOK {
		return true
	}
	if !aOK {
		return false
	}
	var ac, bc string
	if a.DeviceCredential != nil {
		ac = a.DeviceCredential.Credential
	}
	if b.DeviceCredential != nil {
		bc = b.DeviceCredential.Credential
	}
	return ac != bc || mobileOfficialRelayOrigin(a) != mobileOfficialRelayOrigin(b) || a.EffectiveState() != b.EffectiveState()
}

var _ engine.RelayAuthProvider = (*mobileCloudRelay)(nil)

type finishCloudLoginDTO struct {
	CloudURL        string `json:"cloud_url,omitempty"`
	Code            string `json:"code"`
	CodeVerifier    string `json:"code_verifier"`
	RedirectURI     string `json:"redirect_uri"`
	DeviceName      string `json:"device_name,omitempty"`
	AutoEnableRelay bool   `json:"auto_enable_relay,omitempty"`
}

// GetCloudStatus returns the secret-free Cloud session StatusSnapshot as JSON.
func (m *Engine) GetCloudStatus() (string, error) {
	if m == nil || m.cloudRelay == nil || m.cloudRelay.mgr == nil {
		return marshalJSON(cloudauth.StatusSnapshot{State: "signed_out"})
	}
	st, err := m.cloudRelay.mgr.Status()
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(st)
}

// FinishCloudLogin completes PKCE code exchange in Go, persists cloud-session.json
// without exposing tokens to callers, optionally enables official hybrid relay
// and provisions a scoped device credential, then returns StatusSnapshot JSON.
func (m *Engine) FinishCloudLogin(reqJSON string) (string, error) {
	if m == nil || m.cloudRelay == nil || m.cloudRelay.mgr == nil {
		return "", wrapErr(errors.New("cloud relay controller not initialized"))
	}
	var req finishCloudLoginDTO
	if err := json.Unmarshal([]byte(reqJSON), &req); err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeInvalidArgument, Message: "invalid cloud login json"})
	}
	deviceName := strings.TrimSpace(req.DeviceName)
	if deviceName == "" {
		m.settingsMu.Lock()
		deviceName = strings.TrimSpace(m.storedSettings.DeviceName)
		m.settingsMu.Unlock()
	}

	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()

	sess, err := cloudauth.ExchangeCode(ctx, m.cloudRelay.httpCli, req.CloudURL, req.Code, req.CodeVerifier, req.RedirectURI, deviceName)
	if err != nil {
		return "", wrapErr(&engine.Error{Code: engine.CodeUnavailable, Message: err.Error(), Retryable: true})
	}
	if _, err := m.cloudRelay.mgr.SetSession(sess); err != nil {
		return "", wrapErr(err)
	}

	m.settingsMu.Lock()
	if req.AutoEnableRelay {
		m.storedSettings.Sync.Relay.Enabled = true
		_ = saveSettingsAtomic(m.settingsPath, m.redactedStoredSettingsLocked())
		manualKey, manualBound := m.effectiveManualKeyForLocked(m.storedSettings.Sync.Relay.URL)
		m.cloudRelay.setStoredConfig(m.storedSettings, manualKey, m.relayAccessKeyTarget, manualBound)
	}
	officialActive := m.cloudRelay.isOfficialURL(m.storedSettings.Sync.Relay.URL) &&
		m.storedSettings.Sync.Enabled && m.storedSettings.Sync.Relay.Enabled
	m.settingsMu.Unlock()

	if officialActive {
		provCtx, provCancel := context.WithTimeout(context.Background(), 15*time.Second)
		_, _, _ = m.cloudRelay.mgr.EnsureDeviceCredential(provCtx)
		provCancel()
	}

	m.reconcileCloudRelayState(ctx)
	m.e.NotifyRelayAuthChanged()
	m.cloudRelay.notify()

	st, err := m.cloudRelay.mgr.Status()
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(st)
}

// EnsureCloudRelay verifies an active Cloud session, switches to or keeps official
// hybrid relay enabled, provisions a scoped device credential, and returns StatusSnapshot JSON.
func (m *Engine) EnsureCloudRelay() (string, error) {
	if m == nil || m.cloudRelay == nil || m.cloudRelay.mgr == nil {
		return "", wrapErr(errors.New("cloud relay controller not initialized"))
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	sess, ok, err := m.cloudRelay.mgr.Reload()
	if err != nil {
		return "", wrapErr(err)
	}
	if !ok || sess.EffectiveState() != cloudauth.SessionStateActive {
		m.reconcileCloudRelayState(ctx)
		return "", wrapErr(&engine.Error{
			Code:    engine.CodePermissionDenied,
			Message: "cloud sign-in required",
		})
	}

	_, _, provErr := m.cloudRelay.mgr.EnsureDeviceCredential(ctx)
	if provErr != nil {
		m.reconcileCloudRelayState(ctx)
		code := engine.CodeUnavailable
		retryable := true
		updated, _, _ := m.cloudRelay.mgr.Reload()
		if updated.EffectiveState() == cloudauth.SessionStateReauthRequired {
			code = engine.CodePermissionDenied
			retryable = false
		}
		return "", wrapErr(&engine.Error{
			Code:      code,
			Message:   provErr.Error(),
			Retryable: retryable,
		})
	}

	m.settingsMu.Lock()
	if m.cloudRelay.isOfficialURL(m.storedSettings.Sync.Relay.URL) && !m.storedSettings.Sync.Relay.Enabled {
		m.storedSettings.Sync.Relay.Enabled = true
		_ = saveSettingsAtomic(m.settingsPath, m.redactedStoredSettingsLocked())
		manualKey, manualBound := m.effectiveManualKeyForLocked(m.storedSettings.Sync.Relay.URL)
		m.cloudRelay.setStoredConfig(m.storedSettings, manualKey, m.relayAccessKeyTarget, manualBound)
	}
	m.settingsMu.Unlock()

	m.reconcileCloudRelayState(ctx)
	m.e.NotifyRelayAuthChanged()
	m.cloudRelay.notify()

	st, err := m.cloudRelay.mgr.Status()
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(st)
}

// LogoutCloud best-effort revokes scoped device credentials and refresh sessions
// on Cloud, removes cloud-session.json, disables official relay when no manual
// key exists, and returns StatusSnapshot JSON.
func (m *Engine) LogoutCloud() (string, error) {
	if m == nil || m.cloudRelay == nil || m.cloudRelay.mgr == nil {
		return marshalJSON(cloudauth.StatusSnapshot{State: "signed_out"})
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := m.cloudRelay.mgr.Logout(ctx); err != nil {
		return "", wrapErr(err)
	}

	m.reconcileCloudRelayState(ctx)
	m.e.NotifyRelayAuthChanged()
	m.cloudRelay.notify()

	st, err := m.cloudRelay.mgr.Status()
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(st)
}

func (m *Engine) reconcileCloudRelayState(ctx context.Context) {
	if m == nil || m.cloudRelay == nil || m.e == nil {
		return
	}
	m.settingsMu.Lock()
	defer m.settingsMu.Unlock()

	stored := m.storedSettings
	manualKey, manualBound := m.effectiveManualKeyForLocked(stored.Sync.Relay.URL)
	if m.cloudRelay.shouldDisableOfficialRelay(stored, manualKey) {
		stored.Sync.Relay.Enabled = false
	}
	stored.Sync.Relay.AccessKey = ""
	changedStored := stored.Sync.Relay.Enabled != m.storedSettings.Sync.Relay.Enabled
	m.storedSettings = stored
	m.relayURL = stored.Sync.Relay.URL
	m.cloudRelay.setStoredConfig(m.storedSettings, manualKey, m.relayAccessKeyTarget, manualBound)

	if changedStored {
		_ = saveSettingsAtomic(m.settingsPath, m.redactedStoredSettingsLocked())
	}

	guarded := m.cloudRelay.applyRelayGuard(m.storedSettings, manualKey)
	curState, err := m.e.GetState(ctx)
	if err != nil {
		return
	}
	if !changedStored && !relaySettingsDiffer(curState.Settings.Value.Sync.Relay, guarded.Sync.Relay, manualKey) {
		return
	}
	_, _ = m.e.UpdateSettings(ctx, engine.UpdateSettingsRequest{
		ExpectedRevision: curState.Settings.Revision,
		Value:            guarded,
	})
}

func relaySettingsDiffer(cur engine.RelaySyncSettings, next engine.RelaySyncSettings, manualKey string) bool {
	return cur.Enabled != next.Enabled ||
		strings.TrimSpace(cur.URL) != strings.TrimSpace(next.URL) ||
		cur.MaxBytes != next.MaxBytes ||
		cur.HasAccessKey != (strings.TrimSpace(manualKey) != "")
}
