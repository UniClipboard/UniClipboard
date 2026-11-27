// Package cloudauth implements RFC 8252 loopback OAuth 2.0 + PKCE account login
// against UniClipboard Cloud (https://cloud.uniclipboard.app), local session
// persistence, token refresh/revocation, and Cloud feature provisioning (such
// as Hyper Relay API keys).
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
)

const (
	// DefaultCloudURL is the official UniClipboard Cloud origin.
	DefaultCloudURL = "https://cloud.uniclipboard.app"

	sessionFilename    = "cloud-session.json"
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

// Session represents a signed-in UniClipboard Cloud account session for one profile.
// AccessToken and RefreshToken are secrets and must never be logged or echoed.
type Session struct {
	CloudURL          string `json:"cloud_url"`
	AccessToken       string `json:"access_token,omitempty"`
	AccessExpiresAtMs int64  `json:"access_expires_at_ms,omitempty"`
	RefreshToken      string `json:"refresh_token"`
	DeviceName        string `json:"device_name,omitempty"`
	User              User   `json:"user"`
	SignedInAtMs      int64  `json:"signed_in_at_ms"`
	RelayKeyID        string `json:"relay_key_id,omitempty"`
	RelayKeyPrefix    string `json:"relay_key_prefix,omitempty"`
}

// RelayCredential is a Cloud-provisioned Hyper Relay API key and metadata.
// APIKey is a secret and must never be logged or echoed in error messages.
type RelayCredential struct {
	KeyID            string `json:"id"`
	Name             string `json:"name"`
	KeyPrefix        string `json:"key_prefix"`
	APIKey           string `json:"api_key"`
	RelayURL         string `json:"relay_url"`
	MaxTransferBytes int64  `json:"max_transfer_bytes"`
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

// LoadSession reads a persisted UniClipboard Cloud session from profileDir.
// Returns (Session{}, false, nil) when no session exists.
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
	if strings.TrimSpace(s.CloudURL) == "" || strings.TrimSpace(s.RefreshToken) == "" {
		return Session{}, false, nil
	}
	return s, true, nil
}

// SaveSession atomically writes sess to profileDir with owner-only (0600) permissions.
func SaveSession(profileDir string, sess Session) error {
	if strings.TrimSpace(profileDir) == "" {
		return errors.New("profile directory is required")
	}
	if err := os.MkdirAll(profileDir, 0o700); err != nil {
		return err
	}
	raw, err := json.MarshalIndent(sess, "", "  ")
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
			deliver(callbackResult{err: fmt.Errorf("cloud authorization failed: %s", sanitizeToken(errParam))})
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
	resp, err := clientOrDefault(cli).Do(req)
	if err != nil {
		return Session{}, fmt.Errorf("cloud token refresh failed: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxJSONRespBytes))
	if err != nil {
		return Session{}, errors.New("failed to read cloud refresh response")
	}
	if resp.StatusCode != http.StatusOK {
		return Session{}, fmt.Errorf("cloud token refresh failed (http %d)", resp.StatusCode)
	}
	var out struct {
		AccessToken  string `json:"access_token"`
		RefreshToken string `json:"refresh_token"`
		ExpiresIn    int64  `json:"expires_in"`
	}
	if err := json.Unmarshal(body, &out); err != nil || strings.TrimSpace(out.AccessToken) == "" {
		return Session{}, errors.New("cloud token refresh returned invalid response")
	}
	updated := sess
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
	resp, err := clientOrDefault(cli).Do(req)
	if err != nil {
		return fmt.Errorf("cloud session revoke failed: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode >= 500 {
		return fmt.Errorf("cloud session revoke failed (http %d)", resp.StatusCode)
	}
	return nil
}

// CreateRelayAPIKey mints a user-scoped Hyper Relay API key using an existing
// UniClipboard Cloud Session, refreshing the access token first if needed.
func CreateRelayAPIKey(ctx context.Context, cli *http.Client, sess Session, deviceName string) (RelayCredential, Session, error) {
	httpCli := clientOrDefault(cli)
	cur := sess
	needsRefresh := strings.TrimSpace(cur.AccessToken) == "" ||
		(cur.AccessExpiresAtMs > 0 && time.Now().Add(accessRefreshSkew).UnixMilli() >= cur.AccessExpiresAtMs)
	if needsRefresh {
		refreshed, err := Refresh(ctx, httpCli, cur)
		if err != nil {
			return RelayCredential{}, cur, err
		}
		cur = refreshed
	}
	label := resolveDeviceName(firstNonEmpty(deviceName, cur.DeviceName))
	cred, status, err := postRelayAPIKey(ctx, httpCli, cur.CloudURL, cur.AccessToken, label)
	if err != nil && status == http.StatusUnauthorized && strings.TrimSpace(cur.RefreshToken) != "" {
		refreshed, rerr := Refresh(ctx, httpCli, cur)
		if rerr != nil {
			return RelayCredential{}, cur, err
		}
		cur = refreshed
		cred, _, err = postRelayAPIKey(ctx, httpCli, cur.CloudURL, cur.AccessToken, label)
	}
	if err != nil {
		return RelayCredential{}, cur, err
	}
	cur.RelayKeyID = cred.KeyID
	cur.RelayKeyPrefix = cred.KeyPrefix
	return cred, cur, nil
}

// RevokeRelayAPIKey revokes a previously minted Relay API key using sess.
func RevokeRelayAPIKey(ctx context.Context, cli *http.Client, sess Session, keyID string) error {
	keyID = strings.TrimSpace(keyID)
	if keyID == "" {
		return nil
	}
	cloudURL, err := ResolveCloudURL(sess.CloudURL)
	if err != nil {
		return err
	}
	httpCli := clientOrDefault(cli)
	cur := sess
	if strings.TrimSpace(cur.AccessToken) == "" ||
		(cur.AccessExpiresAtMs > 0 && time.Now().Add(accessRefreshSkew).UnixMilli() >= cur.AccessExpiresAtMs) {
		if refreshed, rerr := Refresh(ctx, httpCli, cur); rerr == nil {
			cur = refreshed
		}
	}
	if strings.TrimSpace(cur.AccessToken) == "" {
		return errors.New("cloud session has no access token")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, cloudURL+"/api/relay/v2/api-keys/"+url.PathEscape(keyID), nil)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+cur.AccessToken)
	resp, err := httpCli.Do(req)
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode == http.StatusNotFound || resp.StatusCode == http.StatusNoContent || resp.StatusCode == http.StatusOK {
		return nil
	}
	return fmt.Errorf("cloud relay key revoke failed (http %d)", resp.StatusCode)
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
	resp, err := cli.Do(req)
	if err != nil {
		return Session{}, fmt.Errorf("cloud token exchange failed: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxJSONRespBytes))
	if err != nil {
		return Session{}, errors.New("failed to read cloud token response")
	}
	if resp.StatusCode != http.StatusOK {
		return Session{}, fmt.Errorf("cloud token exchange failed (http %d)", resp.StatusCode)
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

func postRelayAPIKey(ctx context.Context, cli *http.Client, cloudURL, accessToken, deviceName string) (RelayCredential, int, error) {
	payload, err := json.Marshal(map[string]string{
		"name": deviceName,
	})
	if err != nil {
		return RelayCredential{}, 0, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, cloudURL+"/api/relay/v2/api-keys", bytes.NewReader(payload))
	if err != nil {
		return RelayCredential{}, 0, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+accessToken)
	resp, err := cli.Do(req)
	if err != nil {
		return RelayCredential{}, 0, fmt.Errorf("cloud relay api key request failed: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxJSONRespBytes))
	if err != nil {
		return RelayCredential{}, resp.StatusCode, errors.New("failed to read cloud relay api key response")
	}
	if resp.StatusCode != http.StatusCreated && resp.StatusCode != http.StatusOK {
		if resp.StatusCode == http.StatusTooManyRequests {
			return RelayCredential{}, resp.StatusCode, errors.New("cloud relay api key limit reached; revoke an unused key in UniClipboard Cloud")
		}
		return RelayCredential{}, resp.StatusCode, fmt.Errorf("cloud relay api key creation failed (http %d)", resp.StatusCode)
	}
	var cred RelayCredential
	if err := json.Unmarshal(body, &cred); err != nil || strings.TrimSpace(cred.APIKey) == "" {
		return RelayCredential{}, resp.StatusCode, errors.New("cloud returned invalid relay api key")
	}
	cred.APIKey = strings.TrimSpace(cred.APIKey)
	cred.RelayURL = strings.TrimSpace(cred.RelayURL)
	return cred, resp.StatusCode, nil
}

func clientOrDefault(cli *http.Client) *http.Client {
	if cli != nil {
		return cli
	}
	return &http.Client{Timeout: defaultHTTPTimeout}
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

func sanitizeToken(raw string) string {
	var b strings.Builder
	for _, r := range raw {
		if (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9') || r == '_' || r == '-' {
			b.WriteRune(r)
		}
		if b.Len() >= 64 {
			break
		}
	}
	if b.Len() == 0 {
		return "unknown"
	}
	return b.String()
}

func writeCallbackHTML(w http.ResponseWriter, status int, title, body string) {
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(status)
	_, _ = fmt.Fprintf(w, `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>%s</title><style>body{font-family:-apple-system,BlinkMacSystemFont,Segoe UI,sans-serif;max-width:460px;margin:18vh auto;padding:0 20px;color:#111;line-height:1.5}h1{font-size:20px;margin:0 0 8px}p{color:#555;margin:0}</style></head><body><h1>%s</h1><p>%s</p></body></html>`,
		html.EscapeString(title), html.EscapeString(title), html.EscapeString(body))
}
