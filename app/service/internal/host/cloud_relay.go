package host

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/fsnotify/fsnotify"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/cloudauth"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/relayclient"
	"github.com/uniclipboard/unic/internal/relaycrypto"
)

const (
	cloudSessionDebounce  = 100 * time.Millisecond
	cloudRotateMinDelay   = 100 * time.Millisecond
	cloudRotateMinBackoff = 500 * time.Millisecond
	cloudRotateMaxBackoff = 5 * time.Minute
	cloudCredentialSkew   = 60 * time.Second
)

// cloudRelayController bridges UniClipboard Cloud managed device credentials
// with Engine.RelayAuthProvider, keeping secrets in cloud-session.json (never
// writing managed credentials into config.toml), silently rotating before
// rotate_after_ms, watching cross-process session updates, revoking/clearing on
// transitions off the official origin, re-issuing before return-to-official first
// requests, and disabling official relay when signed out without fallback.
type cloudRelayController struct {
	n     *Node
	dir   string
	mgr   *cloudauth.Manager
	wake  chan struct{}
	nowFn func() time.Time

	opMu      sync.Mutex
	disableMu sync.Mutex

	stateMu         sync.Mutex
	engineRelayHeld bool
	engineRelayURL  string
	pendingRetryAt  time.Time
}

func (c *cloudRelayController) isEngineRelayHeld() bool {
	c.stateMu.Lock()
	defer c.stateMu.Unlock()
	return c.engineRelayHeld
}

func (c *cloudRelayController) getEngineRelayURL() string {
	c.stateMu.Lock()
	defer c.stateMu.Unlock()
	return c.engineRelayURL
}

func (c *cloudRelayController) setEngineRelayState(held bool, targetURL string) {
	c.stateMu.Lock()
	c.engineRelayHeld = held
	c.engineRelayURL = strings.TrimSpace(targetURL)
	c.stateMu.Unlock()
}

func (c *cloudRelayController) setPendingRetryAt(at time.Time) {
	c.stateMu.Lock()
	c.pendingRetryAt = at
	c.stateMu.Unlock()
}

func (c *cloudRelayController) takePendingRetryAt() time.Time {
	c.stateMu.Lock()
	at := c.pendingRetryAt
	c.pendingRetryAt = time.Time{}
	c.stateMu.Unlock()
	return at
}

func newCloudRelayController(n *Node, dir string, cfgMgr *config.Manager) *cloudRelayController {
	c := &cloudRelayController{
		n:     n,
		dir:   strings.TrimSpace(dir),
		wake:  make(chan struct{}, 1),
		nowFn: time.Now,
	}
	c.mgr = cloudauth.NewManager(c.dir, cloudauth.ManagerOptions{
		DeviceName: func() string { return strings.TrimSpace(cfgMgr.Current().DeviceName) },
		OnRevoked:  c.handleAuthoritativeRevocation,
	})
	return c
}

// AccessKey implements engine.RelayAuthProvider.
func (c *cloudRelayController) AccessKey(origin string) (string, bool) {
	if c == nil || c.mgr == nil || c.n == nil || c.n.cfg == nil {
		return "", false
	}
	cfg := c.n.cfg.Current()
	if !cfg.Sync.Enabled || !cfg.Sync.Relay.Enabled {
		return "", false
	}
	normOrigin, err := relaycrypto.NormalizeRelayOrigin(origin)
	if err != nil {
		return "", false
	}
	if !c.isOfficialMode(cfg) {
		if normOrigin != c.allowedStaticFallbackOrigin(cfg) {
			return "", false
		}
		return effectiveStaticRelayKey(cfg), true
	}
	sess, ok := c.mgr.Current()
	if ok && sess.HasActiveCredential(c.nowFn()) {
		expected := officialRelayOrigin(sess)
		if expected != "" && normOrigin == expected {
			return strings.TrimSpace(sess.DeviceCredential.Credential), true
		}
		// Never forward static fallback credentials to a different/stale origin
		// while a managed credential is active, or to an untrusted origin.
		return "", false
	}
	// Static fallback credentials may only be sent to an explicitly configured
	// origin (sync.relay.url / UNIC_RELAY_URL) or the fixed default official
	// entry when unconfigured — never to an expired Cloud-advertised origin.
	if k := effectiveStaticRelayKey(cfg); k != "" && normOrigin == c.allowedStaticFallbackOrigin(cfg) {
		return k, true
	}
	return "", false
}

// OnUnauthorized implements engine.RelayAuthProvider.
func (c *cloudRelayController) OnUnauthorized(ctx context.Context, origin, rejectedKey string) bool {
	if c == nil || c.mgr == nil || c.n == nil || c.n.cfg == nil {
		return false
	}
	c.opMu.Lock()
	defer c.opMu.Unlock()

	cfg := c.n.cfg.Current()
	if !cfg.Sync.Enabled || !cfg.Sync.Relay.Enabled || !c.isOfficialMode(cfg) {
		return false
	}
	sess, ok := c.mgr.Current()
	if !ok || sess.EffectiveState() != cloudauth.SessionStateActive {
		return false
	}
	normOrigin, err := relaycrypto.NormalizeRelayOrigin(origin)
	if err != nil {
		return false
	}
	expected := officialRelayOrigin(sess)
	if sess.DeviceCredential != nil && strings.TrimSpace(sess.DeviceCredential.RelayURL) != "" {
		if normOrigin != expected {
			return false
		}
	} else if normOrigin != relayclient.DefaultURL && strings.TrimSpace(os.Getenv("UNIC_CLOUD_URL")) == "" {
		return false
	}

	rotated, updated, _ := c.mgr.HandleRelayUnauthorized(ctx, rejectedKey)
	if !rotated {
		if !updated.HasActiveCredential(c.nowFn()) {
			c.notify()
		}
		return false
	}
	if updated.DeviceCredential == nil || officialRelayOrigin(updated) != normOrigin {
		c.notify()
		return false
	}
	c.notify()
	return true
}

func officialRelayOrigin(sess cloudauth.Session) string {
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

func (c *cloudRelayController) allowedStaticFallbackOrigin(cfg config.Config) string {
	if u := strings.TrimSpace(cfg.Sync.Relay.URL); u != "" {
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

func effectiveStaticRelayKey(cfg config.Config) string {
	if k := strings.TrimSpace(cfg.Sync.Relay.AccessKey); k != "" {
		return k
	}
	return strings.TrimSpace(os.Getenv("UNIC_RELAY_ACCESS_KEY"))
}

func hasFallbackRelayKey(cfg config.Config) bool {
	return effectiveStaticRelayKey(cfg) != ""
}

// isOfficialMode reports whether the configured relay target is the official
// hosted relay (explicit empty config.toml url and either default URL,
// matching DeviceCredential.RelayURL, or UNIC_CLOUD_URL test target).
func (c *cloudRelayController) isOfficialMode(cfg config.Config) bool {
	if strings.TrimSpace(cfg.Sync.Relay.URL) != "" {
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
		return normEnv == officialRelayOrigin(sess)
	}
	return false
}

func (c *cloudRelayController) isOfficialAutoProvisionTarget(cfg config.Config) bool {
	return cfg.Sync.Enabled && cfg.Sync.Relay.Enabled && c.isOfficialMode(cfg)
}

func (c *cloudRelayController) shouldDisableOfficialRelayInConfig(cfg config.Config) bool {
	if !c.isOfficialMode(cfg) || !cfg.Sync.Relay.Enabled || hasFallbackRelayKey(cfg) {
		return false
	}
	sess, ok := c.mgr.Current()
	return !ok || sess.EffectiveState() != cloudauth.SessionStateActive
}

// shouldHoldEngineOfficialRelay reports whether Engine's effective official
// relay must be held inactive because no active managed credential is currently
// available. Unlike shouldDisableOfficialRelayInConfig, temporary Cloud issuance
// outages while signed in preserve sync.relay.enabled in config.toml so recovery
// resumes automatically without user intervention.
func (c *cloudRelayController) shouldHoldEngineOfficialRelay(cfg config.Config) bool {
	if !c.isOfficialMode(cfg) || !cfg.Sync.Relay.Enabled || hasFallbackRelayKey(cfg) {
		return false
	}
	sess, ok := c.mgr.Current()
	return !ok || !sess.HasActiveCredential(c.nowFn())
}

func (c *cloudRelayController) desiredOfficialEngineURL(cfg config.Config, sess cloudauth.Session, ok bool, now time.Time) string {
	if strings.TrimSpace(cfg.Sync.Relay.URL) != "" {
		return strings.TrimSpace(cfg.Sync.Relay.URL)
	}
	if envURL := strings.TrimSpace(os.Getenv("UNIC_RELAY_URL")); envURL != "" {
		return envURL
	}
	if c.isOfficialMode(cfg) && ok && sess.HasActiveCredential(now) {
		if origin := officialRelayOrigin(sess); origin != "" && origin != relayclient.DefaultURL {
			return origin
		}
	}
	return ""
}

func (c *cloudRelayController) applyRelayGuard(cfg config.Config, settings *engine.Settings) {
	if c == nil || settings == nil {
		return
	}
	held := c.shouldHoldEngineOfficialRelay(cfg)
	if held {
		settings.Sync.Relay.Enabled = false
	} else if c.isOfficialMode(cfg) && strings.TrimSpace(settings.Sync.Relay.URL) == "" {
		if sess, ok := c.mgr.Current(); ok && sess.HasActiveCredential(c.nowFn()) {
			if origin := officialRelayOrigin(sess); origin != "" && origin != relayclient.DefaultURL {
				settings.Sync.Relay.URL = origin
			}
		}
	}
	c.setEngineRelayState(held, settings.Sync.Relay.URL)
}

// reconcileSync synchronizes Cloud device credentials with the current relay
// configuration before Engine consumes updated settings or begins requests.
// When persistConfig is true (safe only when config.Manager.ioMu is not held),
// signed-out official mode without fallback also disables sync.relay.enabled
// directly in config.toml.
func (c *cloudRelayController) reconcileSync(ctx context.Context, persistConfig bool) {
	if c == nil || c.mgr == nil || c.n == nil || c.n.cfg == nil {
		return
	}
	var needDisable bool
	c.opMu.Lock()
	cfg := c.n.cfg.Current()
	sess, ok, err := c.mgr.Reload()
	if err == nil {
		if !c.isOfficialMode(cfg) {
			c.setPendingRetryAt(time.Time{})
			if ok && sess.EffectiveState() == cloudauth.SessionStateActive && sess.DeviceCredential != nil {
				revCtx, cancel := context.WithTimeout(ctx, 3*time.Second)
				_ = c.mgr.ClearDeviceCredential(revCtx)
				cancel()
			}
		} else {
			if ok && sess.EffectiveState() == cloudauth.SessionStateActive && cfg.Sync.Enabled && cfg.Sync.Relay.Enabled && sess.NeedsRotation(c.nowFn()) {
				issCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
				_, _, ierr := c.mgr.EnsureDeviceCredential(issCtx)
				cancel()
				if ierr != nil {
					c.setPendingRetryAt(c.nowFn().Add(cloudRotateMinBackoff))
				} else {
					c.setPendingRetryAt(time.Time{})
				}
			} else {
				c.setPendingRetryAt(time.Time{})
			}
			sess, ok = c.mgr.Current()
			if cfg.Sync.Relay.Enabled && !hasFallbackRelayKey(cfg) && (!ok || sess.EffectiveState() != cloudauth.SessionStateActive) {
				needDisable = true
			}
		}
	}
	c.opMu.Unlock()

	if needDisable {
		if persistConfig {
			c.disableOfficialRelayIfNoFallback()
		} else {
			c.notify()
		}
	}
}

// reconcileAfterSave persists sync.relay.enabled=false after PUT /v1/config
// releases config.Manager locks so callers immediately observe the guarded state.
func (c *cloudRelayController) reconcileAfterSave(ctx context.Context) bool {
	if c == nil || c.mgr == nil || c.n == nil || c.n.cfg == nil {
		return false
	}
	c.opMu.Lock()
	_, _, _ = c.mgr.Reload()
	suppress := c.shouldDisableOfficialRelayInConfig(c.n.cfg.Current())
	c.opMu.Unlock()
	if !suppress {
		return false
	}
	return c.disableOfficialRelayIfNoFallback()
}

func (c *cloudRelayController) notify() {
	if c == nil {
		return
	}
	select {
	case c.wake <- struct{}{}:
	default:
	}
}

func (c *cloudRelayController) handleAuthoritativeRevocation() {
	if c == nil || c.n == nil {
		return
	}
	if c.n.diagnostics != nil {
		c.n.diagnostics.Logger().Warn("cloud.session.reauth_required")
	}
	c.n.hub.emit(api.Event{Kind: "config", Info: "cloud session requires re-authentication"})
	c.notify()
}

func (c *cloudRelayController) disableOfficialRelayIfNoFallback() bool {
	if c == nil || c.n == nil || c.n.cfg == nil {
		return false
	}
	if strings.TrimSpace(os.Getenv("UNIC_RELAY_ACCESS_KEY")) != "" {
		return false
	}
	c.disableMu.Lock()
	defer c.disableMu.Unlock()

	for i := 0; i < 5; i++ {
		st := c.n.cfg.State()
		cur := c.n.cfg.Current()
		if !c.isOfficialMode(cur) ||
			strings.TrimSpace(cur.Sync.Relay.AccessKey) != "" ||
			!cur.Sync.Relay.Enabled {
			return false
		}
		raw, err := config.Set(cur, "sync.relay.enabled", "false")
		if err != nil {
			return false
		}
		if _, err := c.n.cfg.Replace(st.Revision, raw); err == nil {
			if c.n.cfg.OnChange == nil && c.n.e != nil {
				_ = c.n.pushSettings(context.Background())
			}
			return true
		} else if err != config.ErrConflict {
			return false
		}
	}
	return false
}

func (c *cloudRelayController) runRotationLoop(ctx context.Context) {
	if c == nil || c.mgr == nil {
		return
	}
	backoff := time.Duration(0)
	var nextRetryAt time.Time
	lastOfficialMode := c.isOfficialMode(c.n.cfg.Current())
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
		var needDisable bool
		var needPushSettings bool
		var wantHold bool
		var sess cloudauth.Session
		var ok bool
		var err error

		c.opMu.Lock()
		now := c.nowFn()
		beforeSess, beforeOK := c.mgr.Current()
		sess, ok, err = c.mgr.Reload()
		cfg := c.n.cfg.Current()
		if err == nil {
			official := c.isOfficialMode(cfg)
			if official != lastOfficialMode || credChanged(beforeSess, beforeOK, sess, ok) {
				backoff = 0
				nextRetryAt = time.Time{}
				c.setPendingRetryAt(time.Time{})
				lastOfficialMode = official
				if credChanged(beforeSess, beforeOK, sess, ok) {
					needPushSettings = true
				}
			}
			if pending := c.takePendingRetryAt(); !pending.IsZero() && (nextRetryAt.IsZero() || pending.After(nextRetryAt)) {
				nextRetryAt = pending
				if backoff == 0 {
					backoff = cloudRotateMinBackoff
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
					needPushSettings = true
				}
			} else if ok {
				switch sess.EffectiveState() {
				case cloudauth.SessionStateReauthRequired:
					backoff = 0
					nextRetryAt = time.Time{}
					if cfg.Sync.Relay.Enabled && !hasFallbackRelayKey(cfg) {
						needDisable = true
					}
				case cloudauth.SessionStateActive:
					if c.isOfficialAutoProvisionTarget(cfg) && sess.NeedsRotation(now) {
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
									if cfg.Sync.Relay.Enabled && !hasFallbackRelayKey(cfg) {
										needDisable = true
									}
								} else {
									if backoff == 0 {
										backoff = cloudRotateMinBackoff
									} else if backoff < cloudRotateMaxBackoff {
										backoff *= 2
										if backoff > cloudRotateMaxBackoff {
											backoff = cloudRotateMaxBackoff
										}
									}
									nextRetryAt = c.nowFn().Add(backoff)
								}
							} else {
								backoff = 0
								nextRetryAt = time.Time{}
								if rotated {
									needPushSettings = true
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
				if cfg.Sync.Relay.Enabled && !hasFallbackRelayKey(cfg) {
					needDisable = true
				}
			}
			wantHold = c.shouldHoldEngineOfficialRelay(cfg)
			wantURL := c.desiredOfficialEngineURL(cfg, sess, ok, now)
			if wantHold != c.isEngineRelayHeld() || wantURL != c.getEngineRelayURL() {
				needPushSettings = true
			}
		}
		c.opMu.Unlock()

		if needDisable {
			c.disableOfficialRelayIfNoFallback()
		}
		if needPushSettings && c.n.e != nil {
			_ = c.n.pushSettings(ctx)
			if !wantHold {
				c.n.e.NotifyRelayAuthChanged()
			}
		}

		var waitCh <-chan time.Time
		if d, hasWait := c.nextDelay(c.n.cfg.Current(), sess, ok, backoff, nextRetryAt); hasWait {
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

func (c *cloudRelayController) nextDelay(cfg config.Config, sess cloudauth.Session, ok bool, backoff time.Duration, nextRetryAt time.Time) (time.Duration, bool) {
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

	if backoff > 0 && !nextRetryAt.IsZero() {
		d := nextRetryAt.Sub(now)
		if d < cloudRotateMinDelay {
			d = cloudRotateMinDelay
		}
		consider(d)
	} else if ok && sess.EffectiveState() == cloudauth.SessionStateActive && sess.DeviceCredential != nil && c.isOfficialAutoProvisionTarget(cfg) {
		targetMs := sess.DeviceCredential.RotateAfterMs
		if targetMs <= 0 && sess.DeviceCredential.ExpiresAtMs > 0 {
			targetMs = sess.DeviceCredential.ExpiresAtMs - cloudCredentialSkew.Milliseconds()
		}
		if targetMs > 0 {
			d := time.Duration(targetMs-nowMs) * time.Millisecond
			if d < cloudRotateMinDelay {
				d = cloudRotateMinDelay
			}
			consider(d)
		}
	}

	if !c.isEngineRelayHeld() && c.isOfficialMode(cfg) && cfg.Sync.Relay.Enabled &&
		ok && sess.HasActiveCredential(now) && sess.DeviceCredential.ExpiresAtMs > 0 {
		expAt := time.UnixMilli(sess.DeviceCredential.ExpiresAtMs).Add(time.Millisecond)
		d := expAt.Sub(now)
		if d < 0 {
			d = 0
		}
		consider(d)
	}

	return best, haveBest
}

func (c *cloudRelayController) watchSessionFile(ctx context.Context) {
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
				timer = time.NewTimer(cloudSessionDebounce)
			} else {
				if !timer.Stop() {
					select {
					case <-timer.C:
					default:
					}
				}
				timer.Reset(cloudSessionDebounce)
			}
			fire = timer.C
		case <-fire:
			fire = nil
			prev, prevOK := c.mgr.Current()
			cur, curOK, err := c.mgr.Reload()
			if err == nil {
				if c.shouldDisableOfficialRelayInConfig(c.n.cfg.Current()) {
					c.disableOfficialRelayIfNoFallback()
				}
				if credChanged(prev, prevOK, cur, curOK) && c.n.e != nil {
					_ = c.n.pushSettings(ctx)
					c.n.e.NotifyRelayAuthChanged()
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

func credChanged(a cloudauth.Session, aOK bool, b cloudauth.Session, bOK bool) bool {
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
	return ac != bc || officialRelayOrigin(a) != officialRelayOrigin(b) || a.EffectiveState() != b.EffectiveState()
}

var _ engine.RelayAuthProvider = (*cloudRelayController)(nil)
