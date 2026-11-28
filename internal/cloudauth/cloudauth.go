// Package cloudauth implements RFC 8252 loopback OAuth 2.0 + PKCE account login
// against UniClipboard Cloud (https://cloud.uniclipboard.app), cross-process
// session persistence, and managed scoped device relay credentials.
package cloudauth

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"html"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/relaycrypto"
)

const (
	// DefaultCloudURL is the official UniClipboard Cloud origin.
	DefaultCloudURL = "https://cloud.uniclipboard.app"

	// SessionStateActive indicates the Cloud session holds active tokens.
	SessionStateActive = "active"
	// SessionStateReauthRequired indicates Cloud authoritatively revoked the
	// session; secrets are cleared and user re-authentication is required.
	SessionStateReauthRequired = "reauth_required"

	sessionFilename    = "cloud-session.json"
	sessionLockName    = "cloud-session.lock"
	defaultHTTPTimeout = 20 * time.Second
	maxJSONRespBytes   = 64 << 10
	accessRefreshSkew  = 60 * time.Second
)

// LoginOptions configures Login.
type LoginOptions struct {
	// CloudURL overrides UNIC_CLOUD_URL and DefaultCloudURL when non-empty.
	CloudURL string
	// DeviceName is displayed on the consent screen and associated with the session.
	DeviceName string
	// OpenBrowser is invoked with the authorization URL once the loopback listener
	// is ready. If nil, OpenSystemBrowser is used.
	OpenBrowser func(authURL string) error
	// HTTPClient overrides the HTTP client used for token exchanges.
	HTTPClient *http.Client
}

// User holds non-secret profile metadata for the signed-in UniClipboard Cloud user.
type User struct {
	ClerkUserID string `json:"clerkUserId,omitempty"`
	Email       string `json:"email,omitempty"`
	FirstName   string `json:"firstName,omitempty"`
	LastName    string `json:"lastName,omitempty"`
	ImageURL    string `json:"imageUrl,omitempty"`
}

// DisplayLabel returns a human-readable account label (email, full name, or fallback).
func (u User) DisplayLabel() string {
	if e := strings.TrimSpace(u.Email); e != "" {
		return e
	}
	name := strings.TrimSpace(strings.TrimSpace(u.FirstName) + " " + strings.TrimSpace(u.LastName))
	if name != "" {
		return name
	}
	return "UniClipboard Cloud"
}

// DeviceCredential holds the Cloud-issued short/medium-lived scoped relay
// credential managed automatically for this device session. Credential is a
// secret and must never be logged or echoed in errors, events, or status DTOs.
type DeviceCredential struct {
	ID               string   `json:"id"`
	KeyPrefix        string   `json:"key_prefix"`
	Credential       string   `json:"credential,omitempty"`
	Scopes           []string `json:"scopes,omitempty"`
	RelayURL         string   `json:"relay_url"`
	MaxTransferBytes int64    `json:"max_transfer_bytes"`
	ExpiresAtMs      int64    `json:"expires_at_ms"`
	RotateAfterMs    int64    `json:"rotate_after_ms"`
}

func (c *DeviceCredential) clone() *DeviceCredential {
	if c == nil {
		return nil
	}
	cp := *c
	if len(c.Scopes) > 0 {
		cp.Scopes = append([]string(nil), c.Scopes...)
	}
	return &cp
}

// Session represents a UniClipboard Cloud account session for one profile.
// AccessToken, RefreshToken, and DeviceCredential.Credential are secrets and
// must never be logged or echoed.
type Session struct {
	State             string            `json:"state,omitempty"`
	CloudURL          string            `json:"cloud_url"`
	AccessToken       string            `json:"access_token,omitempty"`
	AccessExpiresAtMs int64             `json:"access_expires_at_ms,omitempty"`
	RefreshToken      string            `json:"refresh_token,omitempty"`
	DeviceName        string            `json:"device_name,omitempty"`
	User              User              `json:"user"`
	SignedInAtMs      int64             `json:"signed_in_at_ms"`
	DeviceCredential  *DeviceCredential `json:"device_credential,omitempty"`
}

// EffectiveState returns SessionStateReauthRequired, SessionStateActive, or ""
// when the session holds neither state.
func (s Session) EffectiveState() string {
	if strings.TrimSpace(s.State) == SessionStateReauthRequired {
		return SessionStateReauthRequired
	}
	if strings.TrimSpace(s.RefreshToken) != "" {
		return SessionStateActive
	}
	return ""
}

// Clone returns a copy of s with any nested DeviceCredential copied.
func (s Session) Clone() Session {
	cp := s
	cp.DeviceCredential = s.DeviceCredential.clone()
	return cp
}

// HasActiveCredential reports whether s holds an active session with an
// unexpired managed relay credential bound to a valid HTTPS origin at now.
func (s Session) HasActiveCredential(now time.Time) bool {
	if s.EffectiveState() != SessionStateActive || s.DeviceCredential == nil {
		return false
	}
	if strings.TrimSpace(s.DeviceCredential.Credential) == "" {
		return false
	}
	if _, err := NormalizeAdvertisedRelayURL(s.DeviceCredential.RelayURL); err != nil {
		return false
	}
	if s.DeviceCredential.ExpiresAtMs > 0 && now.UnixMilli() >= s.DeviceCredential.ExpiresAtMs {
		return false
	}
	return true
}

// NeedsRotation reports whether s has no usable managed credential or has
// reached Cloud's rotate_after_ms window at now.
func (s Session) NeedsRotation(now time.Time) bool {
	if !s.HasActiveCredential(now) {
		return true
	}
	if s.DeviceCredential.RotateAfterMs > 0 && now.UnixMilli() >= s.DeviceCredential.RotateAfterMs {
		return true
	}
	return false
}

// StatusSnapshot is the secret-free public view of the current Cloud session.
type StatusSnapshot struct {
	SignedIn                bool   `json:"signed_in"`
	ReauthRequired          bool   `json:"reauth_required,omitempty"`
	State                   string `json:"state"`
	CloudURL                string `json:"cloud_url,omitempty"`
	Account                 string `json:"account,omitempty"`
	Email                   string `json:"email,omitempty"`
	DeviceName              string `json:"device_name,omitempty"`
	SignedInAtMs            int64  `json:"signed_in_at_ms,omitempty"`
	RelayURL                string `json:"relay_url,omitempty"`
	CredentialPrefix        string `json:"credential_prefix,omitempty"`
	CredentialExpiresAtMs   int64  `json:"credential_expires_at_ms,omitempty"`
	CredentialRotateAfterMs int64  `json:"credential_rotate_after_ms,omitempty"`
}

// Status returns a redacted StatusSnapshot for s.
func (s Session) Status(hasSession bool) StatusSnapshot {
	if !hasSession || s.EffectiveState() == "" {
		return StatusSnapshot{State: "signed_out"}
	}
	out := StatusSnapshot{
		State:        s.EffectiveState(),
		CloudURL:     s.CloudURL,
		Account:      s.User.DisplayLabel(),
		Email:        strings.TrimSpace(s.User.Email),
		DeviceName:   strings.TrimSpace(s.DeviceName),
		SignedInAtMs: s.SignedInAtMs,
	}
	if out.State == SessionStateReauthRequired {
		out.ReauthRequired = true
		return out
	}
	out.SignedIn = true
	if s.DeviceCredential != nil {
		out.RelayURL = strings.TrimSpace(s.DeviceCredential.RelayURL)
		out.CredentialPrefix = strings.TrimSpace(s.DeviceCredential.KeyPrefix)
		out.CredentialExpiresAtMs = s.DeviceCredential.ExpiresAtMs
		out.CredentialRotateAfterMs = s.DeviceCredential.RotateAfterMs
	}
	return out
}

// APIError represents an HTTP error returned by UniClipboard Cloud without
// exposing raw response bodies or secret material.
type APIError struct {
	Status int
	Code   string
	Op     string
}

func (e *APIError) Error() string {
	if e.Code != "" {
		return fmt.Sprintf("%s failed (http %d: %s)", e.Op, e.Status, e.Code)
	}
	return fmt.Sprintf("%s failed (http %d)", e.Op, e.Status)
}

// IsAuthoritativeRevocation reports whether Cloud definitively declared the
// sign-in session revoked or invalid, requiring interactive re-authentication.
func (e *APIError) IsAuthoritativeRevocation() bool {
	if e == nil || e.Status != http.StatusUnauthorized {
		return false
	}
	return e.Code == "invalid_grant" || e.Code == "session_revoked"
}

// NormalizeAdvertisedRelayURL validates an optional Cloud-advertised official
// relay URL. Empty values indicate the fixed default official origin; any
// non-empty value must be a canonical https URL without userinfo, query, or
// fragment. Plain HTTP is never accepted in production or tests.
func NormalizeAdvertisedRelayURL(raw string) (string, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return "", nil
	}
	norm, err := relaycrypto.NormalizeRelayOrigin(trimmed)
	if err != nil {
		return "", errors.New("cloud returned invalid relay URL")
	}
	u, err := url.Parse(norm)
	if err != nil || !strings.EqualFold(u.Scheme, "https") || u.Host == "" {
		return "", errors.New("cloud advertised relay URL must use https")
	}
	return norm, nil
}

// ResolveCloudURL validates and normalizes the target UniClipboard Cloud origin.
func ResolveCloudURL(explicit string) (string, error) {
	raw := strings.TrimSpace(explicit)
	if raw == "" {
		raw = strings.TrimSpace(os.Getenv("UNIC_CLOUD_URL"))
	}
	if raw == "" {
		raw = DefaultCloudURL
	}
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return "", errors.New("cloud URL must be a valid http(s) origin")
	}
	scheme := strings.ToLower(u.Scheme)
	host := strings.ToLower(u.Hostname())
	isLoopback := host == "127.0.0.1" || host == "localhost" || host == "::1"
	if scheme != "https" && (scheme != "http" || !isLoopback) {
		return "", errors.New("cloud URL must use https (or loopback http for local testing)")
	}
	u.Scheme = scheme
	u.Path = strings.TrimRight(u.Path, "/")
	return u.String(), nil
}

// OpenSystemBrowser launches rawURL in the operating system's default browser.
func OpenSystemBrowser(rawURL string) error {
	u, err := url.Parse(strings.TrimSpace(rawURL))
	if err != nil || (u.Scheme != "https" && u.Scheme != "http") || u.Host == "" {
		return errors.New("only http and https URLs are supported")
	}
	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.Command("open", u.String())
	case "windows":
		cmd = exec.Command("rundll32", "url.dll,FileProtocolHandler", u.String())
	default:
		cmd = exec.Command("xdg-open", u.String())
	}
	return cmd.Start()
}

// SessionPath returns the owner-only session file path under a profile directory.
func SessionPath(profileDir string) string {
	return filepath.Join(profileDir, sessionFilename)
}

// SessionLockPath returns the advisory cross-process lock file path under profileDir.
func SessionLockPath(profileDir string) string {
	return filepath.Join(profileDir, sessionLockName)
}

// LoadSession reads a persisted UniClipboard Cloud session from profileDir.
// Returns (Session{}, false, nil) when no valid session exists.
func LoadSession(profileDir string) (Session, bool, error) {
	if strings.TrimSpace(profileDir) == "" {
		return Session{}, false, nil
	}
	raw, err := os.ReadFile(SessionPath(profileDir))
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return Session{}, false, nil
		}
		return Session{}, false, err
	}
	var s Session
	if err := json.Unmarshal(raw, &s); err != nil {
		return Session{}, false, err
	}
	if strings.TrimSpace(s.CloudURL) == "" {
		return Session{}, false, nil
	}
	cloudURL, err := ResolveCloudURL(s.CloudURL)
	if err != nil {
		return Session{}, false, nil
	}
	s.CloudURL = cloudURL
	switch s.EffectiveState() {
	case SessionStateReauthRequired:
		s.State = SessionStateReauthRequired
		s.AccessToken = ""
		s.AccessExpiresAtMs = 0
		s.RefreshToken = ""
		s.DeviceCredential = nil
		return s, true, nil
	case SessionStateActive:
		s.State = SessionStateActive
		if s.DeviceCredential != nil {
			if norm, nerr := NormalizeAdvertisedRelayURL(s.DeviceCredential.RelayURL); nerr != nil {
				s.DeviceCredential = nil
			} else {
				s.DeviceCredential.RelayURL = norm
			}
		}
		return s, true, nil
	default:
		return Session{}, false, nil
	}
}

// SaveSession atomically writes sess to profileDir with owner-only (0600) permissions.
func SaveSession(profileDir string, sess Session) error {
	if strings.TrimSpace(profileDir) == "" {
		return errors.New("profile directory is required")
	}
	if err := os.MkdirAll(profileDir, 0o700); err != nil {
		return err
	}
	cp := sess.Clone()
	if strings.TrimSpace(cp.CloudURL) != "" {
		cloudURL, err := ResolveCloudURL(cp.CloudURL)
		if err != nil {
			return err
		}
		cp.CloudURL = cloudURL
	}
	switch cp.EffectiveState() {
	case SessionStateReauthRequired:
		cp.State = SessionStateReauthRequired
		cp.AccessToken = ""
		cp.AccessExpiresAtMs = 0
		cp.RefreshToken = ""
		cp.DeviceCredential = nil
	case SessionStateActive:
		cp.State = SessionStateActive
		if cp.DeviceCredential != nil {
			if norm, nerr := NormalizeAdvertisedRelayURL(cp.DeviceCredential.RelayURL); nerr != nil {
				cp.DeviceCredential = nil
			} else {
				cp.DeviceCredential.RelayURL = norm
			}
		}
	}
	raw, err := json.MarshalIndent(cp, "", "  ")
	if err != nil {
		return err
	}
	raw = append(raw, '\n')
	tmp, err := os.CreateTemp(profileDir, ".cloud-session-*")
	if err != nil {
		return err
	}
	tmpName := tmp.Name()
	defer func() { _ = os.Remove(tmpName) }()
	if err := tmp.Chmod(0o600); err != nil {
		_ = tmp.Close()
		return err
	}
	if _, err := tmp.Write(raw); err != nil {
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
	return os.Rename(tmpName, SessionPath(profileDir))
}

// DeleteSession removes the persisted Cloud session file if present.
func DeleteSession(profileDir string) error {
	if strings.TrimSpace(profileDir) == "" {
		return nil
	}
	if err := os.Remove(SessionPath(profileDir)); err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return nil
}

// Login runs the RFC 8252 loopback OAuth 2.0 + PKCE flow against UniClipboard
// Cloud and returns the signed-in account Session.
func Login(ctx context.Context, opts LoginOptions) (Session, error) {
	cloudURL, err := ResolveCloudURL(opts.CloudURL)
	if err != nil {
		return Session{}, err
	}
	deviceName := resolveDeviceName(opts.DeviceName)

	verifier, err := randomBase64URL(32) // 43 chars, valid RFC 7636 verifier
	if err != nil {
		return Session{}, err
	}
	challengeSum := sha256.Sum256([]byte(verifier))
	challenge := base64.RawURLEncoding.EncodeToString(challengeSum[:])

	state, err := randomBase64URL(24)
	if err != nil {
		return Session{}, err
	}

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return Session{}, fmt.Errorf("start local callback server: %w", err)
	}
	defer func() { _ = ln.Close() }()

	redirectURI := fmt.Sprintf("http://%s/cb", ln.Addr().String())

	type callbackResult struct {
		code string
		err  error
	}
	resCh := make(chan callbackResult, 1)
	var once sync.Once
	deliver := func(r callbackResult) {
		once.Do(func() { resCh <- r })
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/cb", func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		q := r.URL.Query()
		if errParam := strings.TrimSpace(q.Get("error")); errParam != "" {
			writeCallbackHTML(w, http.StatusBadRequest, "Authorization failed", "UniClipboard Cloud denied or canceled the request.")
			deliver(callbackResult{err: fmt.Errorf("cloud authorization failed: %s", classifyCloudErrorCode(errParam))})
			return
		}
		gotState := q.Get("state")
		code := strings.TrimSpace(q.Get("code"))
		if subtle.ConstantTimeCompare([]byte(gotState), []byte(state)) != 1 || code == "" {
			writeCallbackHTML(w, http.StatusBadRequest, "Invalid callback", "State verification failed. Please retry from UniClipboard.")
			deliver(callbackResult{err: errors.New("cloud callback state mismatch")})
			return
		}
		writeCallbackHTML(w, http.StatusOK, "Signed in to UniClipboard Cloud", "You can close this tab and return to UniClipboard.")
		deliver(callbackResult{code: code})
	})

	srv := &http.Server{
		Handler:           mux,
		ReadHeaderTimeout: 10 * time.Second,
	}
	go func() { _ = srv.Serve(ln) }()
	defer func() {
		shutCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = srv.Shutdown(shutCtx)
	}()

	q := url.Values{}
	q.Set("redirect_uri", redirectURI)
	q.Set("state", state)
	q.Set("code_challenge", challenge)
	q.Set("code_challenge_method", "S256")
	q.Set("device_name", deviceName)
	authURL := cloudURL + "/cli/authorize?" + q.Encode()

	openFn := opts.OpenBrowser
	if openFn == nil {
		openFn = OpenSystemBrowser
	}
	if err := openFn(authURL); err != nil {
		return Session{}, fmt.Errorf("open browser for cloud login: %w", err)
	}

	var code string
	select {
	case <-ctx.Done():
		return Session{}, ctx.Err()
	case cb := <-resCh:
		if cb.err != nil {
			return Session{}, cb.err
		}
		code = cb.code
	}

	return exchangeCode(ctx, clientOrDefault(opts.HTTPClient), cloudURL, code, verifier, redirectURI, deviceName)
}

// Refresh rotates sess's refresh token and obtains a fresh short-lived access token.
func Refresh(ctx context.Context, cli *http.Client, sess Session) (Session, error) {
	cloudURL, err := ResolveCloudURL(sess.CloudURL)
	if err != nil {
		return Session{}, err
	}
	if strings.TrimSpace(sess.RefreshToken) == "" {
		return Session{}, errors.New("cloud session has no refresh token")
	}
	payload, err := json.Marshal(map[string]string{
		"refresh_token": sess.RefreshToken,
	})
	if err != nil {
		return Session{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, cloudURL+"/api/auth/refresh", bytes.NewReader(payload))
	if err != nil {
		return Session{}, err
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := doCloudRequest(cli, cloudURL, req)
	if err != nil {
		return Session{}, errors.New("cloud token refresh request failed")
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxJSONRespBytes))
	if err != nil {
		return Session{}, errors.New("failed to read cloud refresh response")
	}
	if resp.StatusCode != http.StatusOK {
		return Session{}, &APIError{
			Status: resp.StatusCode,
			Code:   parseCloudErrorCode(body),
			Op:     "cloud token refresh",
		}
	}
	var out struct {
		AccessToken  string `json:"access_token"`
		RefreshToken string `json:"refresh_token"`
		ExpiresIn    int64  `json:"expires_in"`
	}
	if err := json.Unmarshal(body, &out); err != nil || strings.TrimSpace(out.AccessToken) == "" {
		return Session{}, errors.New("cloud token refresh returned invalid response")
	}
	updated := sess.Clone()
	updated.State = SessionStateActive
	updated.CloudURL = cloudURL
	updated.AccessToken = strings.TrimSpace(out.AccessToken)
	if strings.TrimSpace(out.RefreshToken) != "" {
		updated.RefreshToken = strings.TrimSpace(out.RefreshToken)
	}
	if out.ExpiresIn > 0 {
		updated.AccessExpiresAtMs = time.Now().Add(time.Duration(out.ExpiresIn) * time.Second).UnixMilli()
	}
	return updated, nil
}

// Logout revokes the Cloud refresh token on the server (best-effort when offline).
func Logout(ctx context.Context, cli *http.Client, sess Session) error {
	cloudURL, err := ResolveCloudURL(sess.CloudURL)
	if err != nil || strings.TrimSpace(sess.RefreshToken) == "" {
		return err
	}
	payload, err := json.Marshal(map[string]string{
		"refresh_token": sess.RefreshToken,
	})
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, cloudURL+"/api/auth/revoke", bytes.NewReader(payload))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := doCloudRequest(cli, cloudURL, req)
	if err != nil {
		return errors.New("cloud session revoke request failed")
	}
	defer func() { _ = resp.Body.Close() }()
	body, _ := io.ReadAll(io.LimitReader(resp.Body, 4096))
	if resp.StatusCode >= 500 {
		return &APIError{
			Status: resp.StatusCode,
			Code:   parseCloudErrorCode(body),
			Op:     "cloud session revoke",
		}
	}
	return nil
}

// IssueDeviceCredential mints or rotates a session-bound scoped relay device
// credential using sess's active access token. Callers coordinating concurrent
// processes should prefer Manager so rotated refresh tokens are saved before
// credential issuance.
func IssueDeviceCredential(ctx context.Context, cli *http.Client, sess Session, deviceName string) (DeviceCredential, Session, error) {
	httpCli := clientOrDefault(cli)
	cur := sess.Clone()
	needsRefresh := strings.TrimSpace(cur.AccessToken) == "" ||
		(cur.AccessExpiresAtMs > 0 && time.Now().Add(accessRefreshSkew).UnixMilli() >= cur.AccessExpiresAtMs)
	if needsRefresh {
		refreshed, err := Refresh(ctx, httpCli, cur)
		if err != nil {
			return DeviceCredential{}, cur, err
		}
		cur = refreshed
	}
	cloudURL, err := ResolveCloudURL(cur.CloudURL)
	if err != nil {
		return DeviceCredential{}, cur, err
	}
	label := resolveDeviceName(firstNonEmpty(deviceName, cur.DeviceName))
	cred, apiErr, err := postDeviceCredential(ctx, httpCli, cloudURL, cur.AccessToken, label)
	if err != nil && apiErr != nil && apiErr.Status == http.StatusUnauthorized && !apiErr.IsAuthoritativeRevocation() && strings.TrimSpace(cur.RefreshToken) != "" {
		refreshed, rerr := Refresh(ctx, httpCli, cur)
		if rerr != nil {
			return DeviceCredential{}, cur, rerr
		}
		cur = refreshed
		cred, _, err = postDeviceCredential(ctx, httpCli, cloudURL, cur.AccessToken, label)
	}
	if err != nil {
		return DeviceCredential{}, cur, err
	}
	cur.State = SessionStateActive
	cur.DeviceCredential = &cred
	return cred, cur, nil
}

// RevokeDeviceCredentials revokes all managed device credentials bound to
// sess's sign-in session family via DELETE /api/v1/device-credentials/relay.
func RevokeDeviceCredentials(ctx context.Context, cli *http.Client, sess Session) error {
	cloudURL, err := ResolveCloudURL(sess.CloudURL)
	if err != nil {
		return err
	}
	httpCli := clientOrDefault(cli)
	cur := sess.Clone()
	if strings.TrimSpace(cur.AccessToken) == "" ||
		(cur.AccessExpiresAtMs > 0 && time.Now().Add(accessRefreshSkew).UnixMilli() >= cur.AccessExpiresAtMs) {
		if refreshed, rerr := Refresh(ctx, httpCli, cur); rerr == nil {
			cur = refreshed
		}
	}
	if strings.TrimSpace(cur.AccessToken) == "" {
		return errors.New("cloud session has no access token")
	}
	_, err = deleteDeviceCredential(ctx, httpCli, cloudURL, cur.AccessToken)
	return err
}

func deleteDeviceCredential(ctx context.Context, cli *http.Client, cloudURL, accessToken string) (*APIError, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, cloudURL+"/api/v1/device-credentials/relay", nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	resp, err := doCloudRequest(cli, cloudURL, req)
	if err != nil {
		return nil, errors.New("cloud device credential revoke request failed")
	}
	defer func() { _ = resp.Body.Close() }()
	body, _ := io.ReadAll(io.LimitReader(resp.Body, maxJSONRespBytes))
	if resp.StatusCode == http.StatusNotFound || resp.StatusCode == http.StatusNoContent || resp.StatusCode == http.StatusOK {
		return nil, nil
	}
	apiErr := &APIError{
		Status: resp.StatusCode,
		Code:   parseCloudErrorCode(body),
		Op:     "cloud device credential revoke",
	}
	return apiErr, apiErr
}

// ExchangeCode exchanges an OAuth 2.0 + PKCE authorization code for a Cloud
// session against cloudURL (or DefaultCloudURL / UNIC_CLOUD_URL when empty)
// after validating the target origin and response origin.
func ExchangeCode(ctx context.Context, cli *http.Client, cloudURL, code, verifier, redirectURI, deviceName string) (Session, error) {
	normCloudURL, err := ResolveCloudURL(cloudURL)
	if err != nil {
		return Session{}, err
	}
	code = strings.TrimSpace(code)
	verifier = strings.TrimSpace(verifier)
	redirectURI = strings.TrimSpace(redirectURI)
	if code == "" || verifier == "" || redirectURI == "" {
		return Session{}, errors.New("missing cloud authorization code or PKCE parameters")
	}
	return exchangeCode(ctx, clientOrDefault(cli), normCloudURL, code, verifier, redirectURI, resolveDeviceName(deviceName))
}

func exchangeCode(ctx context.Context, cli *http.Client, cloudURL, code, verifier, redirectURI, deviceName string) (Session, error) {
	payload, err := json.Marshal(map[string]string{
		"code":          code,
		"code_verifier": verifier,
		"redirect_uri":  redirectURI,
		"device_name":   deviceName,
	})
	if err != nil {
		return Session{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, cloudURL+"/api/auth/token", bytes.NewReader(payload))
	if err != nil {
		return Session{}, err
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := doCloudRequest(cli, cloudURL, req)
	if err != nil {
		return Session{}, errors.New("cloud token exchange request failed")
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxJSONRespBytes))
	if err != nil {
		return Session{}, errors.New("failed to read cloud token response")
	}
	if resp.StatusCode != http.StatusOK {
		return Session{}, &APIError{
			Status: resp.StatusCode,
			Code:   parseCloudErrorCode(body),
			Op:     "cloud token exchange",
		}
	}
	var out struct {
		AccessToken  string `json:"access_token"`
		RefreshToken string `json:"refresh_token"`
		ExpiresIn    int64  `json:"expires_in"`
		User         *User  `json:"user"`
	}
	if err := json.Unmarshal(body, &out); err != nil || strings.TrimSpace(out.AccessToken) == "" || strings.TrimSpace(out.RefreshToken) == "" {
		return Session{}, errors.New("cloud token exchange returned invalid response")
	}
	now := time.Now()
	sess := Session{
		State:        SessionStateActive,
		CloudURL:     cloudURL,
		AccessToken:  strings.TrimSpace(out.AccessToken),
		RefreshToken: strings.TrimSpace(out.RefreshToken),
		DeviceName:   deviceName,
		SignedInAtMs: now.UnixMilli(),
	}
	if out.ExpiresIn > 0 {
		sess.AccessExpiresAtMs = now.Add(time.Duration(out.ExpiresIn) * time.Second).UnixMilli()
	}
	if out.User != nil {
		sess.User = *out.User
	}
	return sess, nil
}

func postDeviceCredential(ctx context.Context, cli *http.Client, cloudURL, accessToken, deviceName string) (DeviceCredential, *APIError, error) {
	payload, err := json.Marshal(map[string]string{
		"device_name": deviceName,
	})
	if err != nil {
		return DeviceCredential{}, nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, cloudURL+"/api/v1/device-credentials/relay", bytes.NewReader(payload))
	if err != nil {
		return DeviceCredential{}, nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+accessToken)
	resp, err := doCloudRequest(cli, cloudURL, req)
	if err != nil {
		return DeviceCredential{}, nil, errors.New("cloud device credential request failed")
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxJSONRespBytes))
	if err != nil {
		return DeviceCredential{}, &APIError{Status: resp.StatusCode, Op: "cloud device credential issue"}, errors.New("failed to read cloud device credential response")
	}
	if resp.StatusCode != http.StatusCreated && resp.StatusCode != http.StatusOK {
		apiErr := &APIError{
			Status: resp.StatusCode,
			Code:   parseCloudErrorCode(body),
			Op:     "cloud device credential issue",
		}
		return DeviceCredential{}, apiErr, apiErr
	}
	var cred DeviceCredential
	if err := json.Unmarshal(body, &cred); err != nil || strings.TrimSpace(cred.Credential) == "" {
		return DeviceCredential{}, &APIError{Status: resp.StatusCode, Op: "cloud device credential issue"}, errors.New("cloud returned invalid device credential")
	}
	normRelayURL, nerr := NormalizeAdvertisedRelayURL(cred.RelayURL)
	if nerr != nil {
		return DeviceCredential{}, &APIError{Status: resp.StatusCode, Op: "cloud device credential issue"}, nerr
	}
	cred.ID = strings.TrimSpace(cred.ID)
	cred.KeyPrefix = strings.TrimSpace(cred.KeyPrefix)
	cred.Credential = strings.TrimSpace(cred.Credential)
	cred.RelayURL = normRelayURL
	return cred, nil, nil
}

var allowedCloudErrorCodes = map[string]bool{
	"invalid_grant":           true,
	"session_revoked":         true,
	"invalid_request":         true,
	"invalid_client":          true,
	"invalid_token":           true,
	"expired_token":           true,
	"access_denied":           true,
	"unauthorized":            true,
	"forbidden":               true,
	"invalid_scope":           true,
	"insufficient_scope":      true,
	"rate_limited":            true,
	"too_many_requests":       true,
	"temporarily_unavailable": true,
	"server_error":            true,
	"service_unavailable":     true,
}

func classifyCloudErrorCode(raw string) string {
	code := strings.ToLower(strings.TrimSpace(raw))
	if code == "" {
		return ""
	}
	if allowedCloudErrorCodes[code] {
		return code
	}
	return "unknown_error"
}

func parseCloudErrorCode(body []byte) string {
	var e struct {
		Error string `json:"error"`
	}
	if err := json.Unmarshal(body, &e); err != nil {
		return ""
	}
	return classifyCloudErrorCode(e.Error)
}

func clientOrDefault(cli *http.Client) *http.Client {
	if cli != nil {
		return cli
	}
	return &http.Client{Timeout: defaultHTTPTimeout}
}

func sameCloudOrigin(expectedCloudURL string, actual *url.URL) bool {
	if actual == nil || actual.Host == "" || actual.User != nil {
		return false
	}
	base, err := url.Parse(expectedCloudURL)
	if err != nil || base.Host == "" {
		return false
	}
	if canonicalCloudOrigin(actual) != canonicalCloudOrigin(base) {
		return false
	}
	basePath := strings.TrimRight(base.EscapedPath(), "/")
	if basePath == "" {
		return true
	}
	actualPath := actual.EscapedPath()
	return actualPath == basePath || strings.HasPrefix(actualPath, basePath+"/")
}

func canonicalCloudOrigin(u *url.URL) string {
	scheme := strings.ToLower(strings.TrimSpace(u.Scheme))
	host := strings.ToLower(strings.TrimSpace(u.Hostname()))
	port := u.Port()
	if (scheme == "https" && port == "443") || (scheme == "http" && port == "80") {
		port = ""
	}
	if strings.Contains(host, ":") {
		host = "[" + host + "]"
	}
	if port != "" {
		return scheme + "://" + host + ":" + port
	}
	return scheme + "://" + host
}

// doCloudRequest sends req only to the validated expectedCloudURL origin, rejects
// HTTP redirects while stripping credentials, and verifies the final response
// originated from expectedCloudURL before any caller reads or acts on it.
func doCloudRequest(cli *http.Client, expectedCloudURL string, req *http.Request) (*http.Response, error) {
	normCloudURL, err := ResolveCloudURL(expectedCloudURL)
	if err != nil {
		return nil, errors.New("invalid cloud URL")
	}
	if req == nil || req.URL == nil || !sameCloudOrigin(normCloudURL, req.URL) {
		return nil, errors.New("cloud request origin mismatch")
	}
	var c http.Client
	if cli != nil {
		c = *cli
	} else {
		c.Timeout = defaultHTTPTimeout
	}
	c.CheckRedirect = func(nextReq *http.Request, _ []*http.Request) error {
		nextReq.Header.Del("Authorization")
		nextReq.Header.Del("Cookie")
		return errors.New("cloud API redirect disallowed")
	}
	resp, err := c.Do(req)
	if err != nil {
		return nil, errors.New("cloud API transport failed")
	}
	if resp == nil || resp.Request == nil || resp.Request.URL == nil ||
		(resp.StatusCode >= 300 && resp.StatusCode < 400) ||
		!sameCloudOrigin(normCloudURL, resp.Request.URL) {
		if resp != nil && resp.Body != nil {
			_ = resp.Body.Close()
		}
		return nil, errors.New("cloud response origin mismatch")
	}
	return resp, nil
}

func resolveDeviceName(raw string) string {
	deviceName := strings.TrimSpace(raw)
	if deviceName == "" {
		if h, herr := os.Hostname(); herr == nil {
			deviceName = strings.TrimSpace(h)
		}
	}
	if deviceName == "" {
		deviceName = "UniClipboard device"
	}
	if len(deviceName) > 120 {
		deviceName = deviceName[:120]
	}
	return deviceName
}

func firstNonEmpty(values ...string) string {
	for _, v := range values {
		if t := strings.TrimSpace(v); t != "" {
			return t
		}
	}
	return ""
}

func randomBase64URL(nBytes int) (string, error) {
	b := make([]byte, nBytes)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(b), nil
}

func writeCallbackHTML(w http.ResponseWriter, status int, title, body string) {
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(status)
	_, _ = fmt.Fprintf(w, `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>%s</title><style>body{font-family:-apple-system,BlinkMacSystemFont,Segoe UI,sans-serif;max-width:460px;margin:18vh auto;padding:0 20px;color:#111;line-height:1.5}h1{font-size:20px;margin:0 0 8px}p{color:#555;margin:0}</style></head><body><h1>%s</h1><p>%s</p></body></html>`,
		html.EscapeString(title), html.EscapeString(title), html.EscapeString(body))
}
