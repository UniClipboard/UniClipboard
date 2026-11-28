package cloudauth

import (
	"context"
	"errors"
	"net/http"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/rogpeppe/go-internal/lockedfile"
)

const unauthorizedRetryCooldown = 30 * time.Second

// ManagerOptions configures a cross-process Cloud session and device credential manager.
type ManagerOptions struct {
	HTTPClient *http.Client
	Now        func() time.Time
	DeviceName func() string
	OnRevoked  func()
}

// Manager coordinates cloud-session.json persistence, single-use refresh-token
// rotation, silent device credential rotation, and authoritative revocation
// transitions across concurrent processes (unic / unicd) for a single profile.
type Manager struct {
	profileDir string
	httpCli    *http.Client
	now        func() time.Time
	deviceName func() string
	onRevoked  func()

	opMu sync.Mutex // serializes lockedfile + Cloud rotation/refresh/revoke operations

	snapMu    sync.RWMutex // never held across I/O or network calls
	loaded    bool
	has       bool
	sess      Session
	cooldowns map[string]time.Time
}

// NewManager constructs a Manager for profileDir.
func NewManager(profileDir string, opts ManagerOptions) *Manager {
	nowFn := opts.Now
	if nowFn == nil {
		nowFn = time.Now
	}
	devFn := opts.DeviceName
	if devFn == nil {
		devFn = func() string { return "" }
	}
	return &Manager{
		profileDir: strings.TrimSpace(profileDir),
		httpCli:    opts.HTTPClient,
		now:        nowFn,
		deviceName: devFn,
		onRevoked:  opts.OnRevoked,
		cooldowns:  map[string]time.Time{},
	}
}

func (m *Manager) withLock(fn func() error) error {
	if m.profileDir != "" {
		if err := os.MkdirAll(m.profileDir, 0o700); err != nil {
			return err
		}
		unlock, err := lockedfile.MutexAt(SessionLockPath(m.profileDir)).Lock()
		if err != nil {
			return err
		}
		defer unlock()
	}
	m.opMu.Lock()
	defer m.opMu.Unlock()
	return fn()
}

func (m *Manager) publishSnapshot(s Session, has bool) {
	cp := s.Clone()
	m.snapMu.Lock()
	m.loaded = true
	m.has = has
	m.sess = cp
	m.snapMu.Unlock()
}

func (m *Manager) loadLocked() (Session, bool, error) {
	if m.profileDir == "" {
		m.snapMu.RLock()
		s, has := m.sess.Clone(), m.has
		m.snapMu.RUnlock()
		return s, has, nil
	}
	s, ok, err := LoadSession(m.profileDir)
	if err != nil {
		return Session{}, false, err
	}
	m.publishSnapshot(s, ok)
	return s.Clone(), ok, nil
}

func (m *Manager) saveLocked(s Session) error {
	if m.profileDir != "" {
		if err := SaveSession(m.profileDir, s); err != nil {
			return err
		}
	}
	m.publishSnapshot(s, true)
	return nil
}

func (m *Manager) deleteLocked() error {
	if m.profileDir != "" {
		if err := DeleteSession(m.profileDir); err != nil {
			return err
		}
	}
	m.publishSnapshot(Session{}, false)
	return nil
}

// Current returns a non-blocking snapshot of the latest published Cloud session
// (reading disk once at cold start if never loaded). Safe for hot-path
// credential and admission lookup while remote rotation/refresh is in flight.
func (m *Manager) Current() (Session, bool) {
	m.snapMu.RLock()
	if m.loaded {
		s, has := m.sess.Clone(), m.has
		m.snapMu.RUnlock()
		return s, has
	}
	m.snapMu.RUnlock()

	var loadedSess Session
	var loadedOK bool
	var loadErr error
	if m.profileDir != "" {
		loadedSess, loadedOK, loadErr = LoadSession(m.profileDir)
	}

	m.snapMu.Lock()
	defer m.snapMu.Unlock()
	if !m.loaded {
		if m.profileDir == "" || loadErr == nil {
			m.loaded = true
			m.has = loadedOK
			m.sess = loadedSess.Clone()
		}
	}
	return m.sess.Clone(), m.has
}

// Reload acquires the profile lock, re-reads cloud-session.json from disk, and
// refreshes the in-memory snapshot.
func (m *Manager) Reload() (Session, bool, error) {
	var out Session
	var ok bool
	err := m.withLock(func() error {
		var lerr error
		out, ok, lerr = m.loadLocked()
		return lerr
	})
	return out, ok, err
}

// Status returns a secret-free StatusSnapshot synchronized with disk.
func (m *Manager) Status() (StatusSnapshot, error) {
	s, ok, err := m.Reload()
	if err != nil {
		return StatusSnapshot{}, err
	}
	return s.Status(ok), nil
}

// SetSession persists a freshly signed-in Session under the cross-process lock.
func (m *Manager) SetSession(sess Session) (Session, error) {
	cp := sess.Clone()
	if cp.EffectiveState() == SessionStateActive {
		cp.State = SessionStateActive
	}
	err := m.withLock(func() error {
		return m.saveLocked(cp)
	})
	if err != nil {
		return Session{}, err
	}
	return cp.Clone(), nil
}

// EnsureDeviceCredential ensures the active Cloud session holds a valid,
// non-expired scoped relay device credential, rotating through Cloud when needed.
func (m *Manager) EnsureDeviceCredential(ctx context.Context) (DeviceCredential, Session, error) {
	var cred DeviceCredential
	var sess Session
	var revoked bool
	err := m.withLock(func() error {
		cur, ok, lerr := m.loadLocked()
		if lerr != nil {
			return lerr
		}
		if !ok || cur.EffectiveState() != SessionStateActive {
			return errors.New("no active UniClipboard Cloud session")
		}
		if !cur.NeedsRotation(m.now()) && cur.DeviceCredential != nil {
			cred = *cur.DeviceCredential.clone()
			sess = cur
			return nil
		}
		var rerr error
		cred, sess, revoked, rerr = m.provisionLocked(ctx, cur)
		return rerr
	})
	if revoked && m.onRevoked != nil {
		m.onRevoked()
	}
	return cred, sess, err
}

// RotateIfDue checks whether the persisted device credential has entered its
// rotate_after_ms window or expired, and silently rotates it with Cloud if so.
func (m *Manager) RotateIfDue(ctx context.Context) (bool, Session, error) {
	var rotated bool
	var sess Session
	var revoked bool
	err := m.withLock(func() error {
		cur, ok, lerr := m.loadLocked()
		if lerr != nil {
			return lerr
		}
		if !ok || cur.EffectiveState() != SessionStateActive {
			sess = cur
			return nil
		}
		if !cur.NeedsRotation(m.now()) {
			sess = cur
			return nil
		}
		_, updated, rev, perr := m.provisionLocked(ctx, cur)
		revoked = rev
		sess = updated
		if perr == nil {
			rotated = true
		}
		return perr
	})
	if revoked && m.onRevoked != nil {
		m.onRevoked()
	}
	return rotated, sess, err
}

// HandleRelayUnauthorized handles a relay control-plane or SSE 401 response.
// Under lock it re-reads disk so an already-rotated sibling credential is
// reused immediately, applies a 30s per-key cooldown, and otherwise silently
// re-issues the device credential (handling authoritative session revocation).
func (m *Manager) HandleRelayUnauthorized(ctx context.Context, rejectedKey string) (bool, Session, error) {
	var rotated bool
	var sess Session
	var revoked bool
	err := m.withLock(func() error {
		cur, ok, lerr := m.loadLocked()
		if lerr != nil {
			return lerr
		}
		if !ok || cur.EffectiveState() != SessionStateActive {
			sess = cur
			return nil
		}
		now := m.now()
		// Another worker/process may have already rotated to a newer credential.
		if cur.HasActiveCredential(now) && rejectedKey != "" && cur.DeviceCredential.Credential != rejectedKey {
			sess = cur
			rotated = true
			return nil
		}
		if until, exists := m.cooldowns[rejectedKey]; exists && now.Before(until) {
			sess = cur
			return nil
		}
		for k, u := range m.cooldowns {
			if !now.Before(u) {
				delete(m.cooldowns, k)
			}
		}
		m.cooldowns[rejectedKey] = now.Add(unauthorizedRetryCooldown)

		_, updated, rev, perr := m.provisionLocked(ctx, cur)
		revoked = rev
		sess = updated
		if perr == nil {
			rotated = true
		}
		return perr
	})
	if revoked && m.onRevoked != nil {
		m.onRevoked()
	}
	return rotated, sess, err
}

// ClearDeviceCredential revokes any active session-bound device credential
// on Cloud (best-effort) and clears DeviceCredential locally under lock while
// preserving the signed-in OAuth session.
func (m *Manager) ClearDeviceCredential(ctx context.Context) error {
	var revoked bool
	err := m.withLock(func() error {
		cur, ok, lerr := m.loadLocked()
		if lerr != nil || !ok {
			return lerr
		}
		if cur.EffectiveState() != SessionStateActive || cur.DeviceCredential == nil {
			return nil
		}
		updated, rev := m.revokeDeviceCredentialLocked(ctx, cur)
		revoked = rev
		if rev {
			return nil
		}
		updated.DeviceCredential = nil
		return m.saveLocked(updated)
	})
	if revoked && m.onRevoked != nil {
		m.onRevoked()
	}
	return err
}

// Logout revokes the device credentials and Cloud refresh session best-effort
// and deletes the persisted session file under lock.
func (m *Manager) Logout(ctx context.Context) error {
	return m.withLock(func() error {
		cur, ok, lerr := m.loadLocked()
		if lerr != nil {
			return lerr
		}
		if ok && cur.EffectiveState() == SessionStateActive {
			_ = RevokeDeviceCredentials(ctx, m.httpCli, cur)
			_ = Logout(ctx, m.httpCli, cur)
		}
		return m.deleteLocked()
	})
}

func (m *Manager) ensureAccessTokenLocked(ctx context.Context, httpCli *http.Client, cur Session, force bool) (Session, bool, error) {
	now := m.now()
	needsRefresh := force || strings.TrimSpace(cur.AccessToken) == "" ||
		(cur.AccessExpiresAtMs > 0 && now.Add(accessRefreshSkew).UnixMilli() >= cur.AccessExpiresAtMs)
	if !needsRefresh {
		return cur, false, nil
	}
	refreshed, err := Refresh(ctx, httpCli, cur)
	if err != nil {
		if isAuthoritativeRevocation(err) {
			if saved, serr := m.markReauthRequiredLocked(cur); serr == nil {
				return saved, true, err
			}
			return cur, true, err
		}
		return cur, false, err
	}
	// Persist rotated single-use refresh_token before subsequent Cloud calls so
	// a crash or transient failure never loses the fresh refresh token.
	if serr := m.saveLocked(refreshed); serr != nil {
		return refreshed, false, serr
	}
	return refreshed, false, nil
}

func (m *Manager) revokeDeviceCredentialLocked(ctx context.Context, sess Session) (Session, bool) {
	httpCli := clientOrDefault(m.httpCli)
	cur, rev, err := m.ensureAccessTokenLocked(ctx, httpCli, sess.Clone(), false)
	if rev {
		return cur, true
	}
	if err != nil || strings.TrimSpace(cur.AccessToken) == "" {
		return cur, false
	}
	cloudURL, cerr := ResolveCloudURL(cur.CloudURL)
	if cerr != nil {
		return cur, false
	}
	apiErr, derr := deleteDeviceCredential(ctx, httpCli, cloudURL, cur.AccessToken)
	if derr != nil && apiErr != nil && apiErr.IsAuthoritativeRevocation() {
		if saved, serr := m.markReauthRequiredLocked(cur); serr == nil {
			return saved, true
		}
		return cur, true
	}
	if derr != nil && apiErr != nil && apiErr.Status == http.StatusUnauthorized && strings.TrimSpace(cur.RefreshToken) != "" {
		cur, rev, rerr := m.ensureAccessTokenLocked(ctx, httpCli, cur, true)
		if rev {
			return cur, true
		}
		if rerr == nil {
			apiErr, derr = deleteDeviceCredential(ctx, httpCli, cloudURL, cur.AccessToken)
			if derr != nil && apiErr != nil && apiErr.IsAuthoritativeRevocation() {
				if saved, serr := m.markReauthRequiredLocked(cur); serr == nil {
					return saved, true
				}
				return cur, true
			}
		}
	}
	return cur, false
}

func (m *Manager) provisionLocked(ctx context.Context, sess Session) (DeviceCredential, Session, bool, error) {
	httpCli := clientOrDefault(m.httpCli)
	cur, rev, err := m.ensureAccessTokenLocked(ctx, httpCli, sess.Clone(), false)
	if err != nil {
		return DeviceCredential{}, cur, rev, err
	}

	cloudURL, err := ResolveCloudURL(cur.CloudURL)
	if err != nil {
		return DeviceCredential{}, cur, false, err
	}
	label := resolveDeviceName(firstNonEmpty(m.deviceName(), cur.DeviceName))
	cred, apiErr, err := postDeviceCredential(ctx, httpCli, cloudURL, cur.AccessToken, label)
	if err != nil && apiErr != nil && apiErr.IsAuthoritativeRevocation() {
		if saved, serr := m.markReauthRequiredLocked(cur); serr == nil {
			return DeviceCredential{}, saved, true, err
		}
		return DeviceCredential{}, cur, true, err
	}
	if err != nil && apiErr != nil && apiErr.Status == http.StatusUnauthorized && strings.TrimSpace(cur.RefreshToken) != "" {
		var rerr error
		cur, rev, rerr = m.ensureAccessTokenLocked(ctx, httpCli, cur, true)
		if rerr != nil {
			return DeviceCredential{}, cur, rev, rerr
		}
		cred, apiErr, err = postDeviceCredential(ctx, httpCli, cloudURL, cur.AccessToken, label)
		if err != nil && apiErr != nil && apiErr.IsAuthoritativeRevocation() {
			if saved, serr := m.markReauthRequiredLocked(cur); serr == nil {
				return DeviceCredential{}, saved, true, err
			}
			return DeviceCredential{}, cur, true, err
		}
	}
	if err != nil {
		return DeviceCredential{}, cur, false, err
	}
	cur.State = SessionStateActive
	cur.DeviceCredential = cred.clone()
	if serr := m.saveLocked(cur); serr != nil {
		return DeviceCredential{}, cur, false, serr
	}
	return cred, cur, false, nil
}

func (m *Manager) markReauthRequiredLocked(sess Session) (Session, error) {
	revokedSess := Session{
		State:        SessionStateReauthRequired,
		CloudURL:     sess.CloudURL,
		DeviceName:   sess.DeviceName,
		User:         sess.User,
		SignedInAtMs: sess.SignedInAtMs,
	}
	err := m.saveLocked(revokedSess)
	return revokedSess, err
}

func isAuthoritativeRevocation(err error) bool {
	var apiErr *APIError
	return errors.As(err, &apiErr) && apiErr.IsAuthoritativeRevocation()
}
