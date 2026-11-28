package cloudauth

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func TestResolveCloudURL(t *testing.T) {
	t.Setenv("UNIC_CLOUD_URL", "")
	u, err := ResolveCloudURL("")
	if err != nil || u != DefaultCloudURL {
		t.Fatalf("default = (%q, %v), want %q", u, err, DefaultCloudURL)
	}
	u, err = ResolveCloudURL("https://cloud.example.com/")
	if err != nil || u != "https://cloud.example.com" {
		t.Fatalf("https = (%q, %v)", u, err)
	}
	u, err = ResolveCloudURL("http://127.0.0.1:3000/")
	if err != nil || u != "http://127.0.0.1:3000" {
		t.Fatalf("loopback = (%q, %v)", u, err)
	}
	for _, bad := range []string{"http://cloud.example.com", "ftp://cloud.example.com", "https://user:pass@cloud.example.com", "https://cloud.example.com?a=1"} {
		if _, err := ResolveCloudURL(bad); err == nil {
			t.Fatalf("expected error for %q", bad)
		}
	}
}

func TestCloudAccountLifecycleAndRelayProvisioning(t *testing.T) {
	var mu sync.Mutex
	var savedChallenge, savedRedirect string
	var revokedRefresh string
	var revokedDeviceCred bool

	cloud := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/cli/authorize":
			q := r.URL.Query()
			if q.Get("code_challenge_method") != "S256" || q.Get("device_name") != "My MacBook" {
				http.Error(w, "bad authorize params", http.StatusBadRequest)
				return
			}
			mu.Lock()
			savedChallenge = q.Get("code_challenge")
			savedRedirect = q.Get("redirect_uri")
			mu.Unlock()
			target, err := url.Parse(q.Get("redirect_uri"))
			if err != nil {
				http.Error(w, "bad redirect", http.StatusBadRequest)
				return
			}
			rq := target.Query()
			rq.Set("code", "auth-code-123")
			rq.Set("state", q.Get("state"))
			target.RawQuery = rq.Encode()
			http.Redirect(w, r, target.String(), http.StatusSeeOther)

		case r.URL.Path == "/api/auth/token":
			var body struct {
				Code         string `json:"code"`
				CodeVerifier string `json:"code_verifier"`
				RedirectURI  string `json:"redirect_uri"`
				DeviceName   string `json:"device_name"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				http.Error(w, "bad json", http.StatusBadRequest)
				return
			}
			sum := sha256.Sum256([]byte(body.CodeVerifier))
			gotChallenge := base64.RawURLEncoding.EncodeToString(sum[:])
			mu.Lock()
			wantChallenge, wantRedirect := savedChallenge, savedRedirect
			mu.Unlock()
			if body.Code != "auth-code-123" || gotChallenge != wantChallenge || body.RedirectURI != wantRedirect {
				http.Error(w, "pkce mismatch", http.StatusBadRequest)
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token":  "jwt-access-1",
				"refresh_token": "opaque-refresh-1",
				"token_type":    "Bearer",
				"expires_in":    900,
				"user": map[string]any{
					"clerkUserId": "user_1",
					"email":       "alice@example.com",
					"firstName":   "Alice",
				},
			})

		case r.URL.Path == "/api/auth/refresh":
			var body struct {
				RefreshToken string `json:"refresh_token"`
			}
			_ = json.NewDecoder(r.Body).Decode(&body)
			if body.RefreshToken != "opaque-refresh-1" {
				w.WriteHeader(http.StatusUnauthorized)
				_ = json.NewEncoder(w).Encode(map[string]string{"error": "invalid_grant"})
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token":  "jwt-access-2",
				"refresh_token": "opaque-refresh-2",
				"expires_in":    900,
			})

		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodPost:
			if r.Header.Get("Authorization") != "Bearer jwt-access-2" {
				http.Error(w, "unauthorized", http.StatusUnauthorized)
				return
			}
			var body struct {
				DeviceName string `json:"device_name"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil || body.DeviceName != "My MacBook" {
				http.Error(w, "bad credential req", http.StatusBadRequest)
				return
			}
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":                 "dc-uuid-1",
				"key_prefix":         "uc_dc_12345678",
				"credential":         "uc_dc_1234567890abcdef",
				"scopes":             []string{"relay"},
				"relay_url":          "https://relay.uniclipboard.app",
				"max_transfer_bytes": 26214400,
				"expires_at_ms":      time.Now().Add(24 * time.Hour).UnixMilli(),
				"rotate_after_ms":    time.Now().Add(12 * time.Hour).UnixMilli(),
			})

		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodDelete:
			mu.Lock()
			revokedDeviceCred = true
			mu.Unlock()
			w.WriteHeader(http.StatusNoContent)

		case r.URL.Path == "/api/auth/revoke":
			var body struct {
				RefreshToken string `json:"refresh_token"`
			}
			_ = json.NewDecoder(r.Body).Decode(&body)
			mu.Lock()
			revokedRefresh = body.RefreshToken
			mu.Unlock()
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(`{"revoked":true}`))

		default:
			http.NotFound(w, r)
		}
	}))
	defer cloud.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	sess, err := Login(ctx, LoginOptions{
		CloudURL:   cloud.URL,
		DeviceName: "My MacBook",
		OpenBrowser: func(authURL string) error {
			go func() {
				resp, err := cloud.Client().Get(authURL)
				if err == nil {
					_ = resp.Body.Close()
				}
			}()
			return nil
		},
		HTTPClient: cloud.Client(),
	})
	if err != nil {
		t.Fatalf("Login: %v", err)
	}
	if sess.User.Email != "alice@example.com" || sess.RefreshToken != "opaque-refresh-1" {
		t.Fatalf("unexpected session: %+v", sess)
	}

	// Simulate expired access token -> IssueDeviceCredential must auto-refresh and rotate.
	sess.AccessExpiresAtMs = time.Now().Add(-time.Minute).UnixMilli()
	cred, updated, err := IssueDeviceCredential(ctx, cloud.Client(), sess, "")
	if err != nil {
		t.Fatalf("IssueDeviceCredential: %v", err)
	}
	if cred.Credential != "uc_dc_1234567890abcdef" || updated.RefreshToken != "opaque-refresh-2" || updated.DeviceCredential == nil || updated.DeviceCredential.ID != "dc-uuid-1" {
		t.Fatalf("unexpected cred/session: cred=%+v sess=%+v", cred, updated)
	}

	// Persist and reload session.
	dir := t.TempDir()
	if err := SaveSession(dir, updated); err != nil {
		t.Fatalf("SaveSession: %v", err)
	}
	loaded, ok, err := LoadSession(dir)
	if err != nil || !ok || loaded.User.DisplayLabel() != "alice@example.com" || loaded.DeviceCredential == nil || loaded.DeviceCredential.KeyPrefix != "uc_dc_12345678" {
		t.Fatalf("LoadSession = (%+v, %v, %v)", loaded, ok, err)
	}

	if err := RevokeDeviceCredentials(ctx, cloud.Client(), loaded); err != nil {
		t.Fatalf("RevokeDeviceCredentials: %v", err)
	}
	if err := Logout(ctx, cloud.Client(), loaded); err != nil {
		t.Fatalf("Logout: %v", err)
	}
	if err := DeleteSession(dir); err != nil {
		t.Fatalf("DeleteSession: %v", err)
	}
	if _, ok, err := LoadSession(dir); err != nil || ok {
		t.Fatalf("expected deleted session, got ok=%v err=%v", ok, err)
	}
	mu.Lock()
	defer mu.Unlock()
	if !revokedDeviceCred || revokedRefresh != "opaque-refresh-2" {
		t.Fatalf("revocations = (device=%v, refresh=%q)", revokedDeviceCred, revokedRefresh)
	}
}

func TestLoginStateMismatch(t *testing.T) {
	cloud := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		target, _ := url.Parse(r.URL.Query().Get("redirect_uri"))
		rq := target.Query()
		rq.Set("code", "abc")
		rq.Set("state", "wrong-state")
		target.RawQuery = rq.Encode()
		http.Redirect(w, r, target.String(), http.StatusSeeOther)
	}))
	defer cloud.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	_, err := Login(ctx, LoginOptions{
		CloudURL: cloud.URL,
		OpenBrowser: func(authURL string) error {
			go func() {
				resp, err := cloud.Client().Get(authURL)
				if err == nil {
					_ = resp.Body.Close()
				}
			}()
			return nil
		},
		HTTPClient: cloud.Client(),
	})
	if err == nil || !strings.Contains(err.Error(), "state mismatch") {
		t.Fatalf("expected state mismatch error, got %v", err)
	}
}

func TestNormalizeAdvertisedRelayURLAndPlainHTTPRejection(t *testing.T) {
	if got, err := NormalizeAdvertisedRelayURL(""); err != nil || got != "" {
		t.Fatalf("empty URL: got (%q, %v)", got, err)
	}
	if got, err := NormalizeAdvertisedRelayURL("https://RELAY.Stage.Example:443/"); err != nil || got != "https://relay.stage.example" {
		t.Fatalf("canonical HTTPS URL: got (%q, %v)", got, err)
	}
	for _, bad := range []string{
		"http://127.0.0.1:8080",
		"http://relay.example.com",
		"ftp://relay.example.com",
		"https://user:pass@relay.example.com",
		"https://relay.example.com?q=1",
	} {
		if got, err := NormalizeAdvertisedRelayURL(bad); err == nil {
			t.Fatalf("expected error for non-HTTPS/invalid URL %q, got %q", bad, got)
		}
	}

	cloud := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1/device-credentials/relay" {
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":         "dc-http",
				"key_prefix": "uc_dc_http",
				"credential": "uc_dc_http_secret",
				"relay_url":  "http://127.0.0.1:9999",
			})
			return
		}
		http.NotFound(w, r)
	}))
	defer cloud.Close()

	mgr := NewManager(t.TempDir(), ManagerOptions{HTTPClient: cloud.Client()})
	if _, err := mgr.SetSession(Session{
		State:             SessionStateActive,
		CloudURL:          cloud.URL,
		AccessToken:       "jwt-ok",
		AccessExpiresAtMs: time.Now().Add(time.Hour).UnixMilli(),
		RefreshToken:      "refresh-ok",
		User:              User{Email: "alice@example.com"},
		SignedInAtMs:      1764200000000,
	}); err != nil {
		t.Fatalf("SetSession: %v", err)
	}
	if _, _, err := mgr.EnsureDeviceCredential(context.Background()); err == nil {
		t.Fatal("expected EnsureDeviceCredential to reject plain HTTP relay_url from Cloud")
	}
}

type fakeCloudOriginRoundTripper struct {
	base           http.RoundTripper
	spoofURL       string
	spoofStatus    int
	spoofBody      string
	redirectTarget string
}

func (f *fakeCloudOriginRoundTripper) RoundTrip(req *http.Request) (*http.Response, error) {
	if f.redirectTarget != "" {
		return &http.Response{
			StatusCode: http.StatusFound,
			Header:     http.Header{"Location": []string{f.redirectTarget}},
			Body:       http.NoBody,
			Request:    req,
		}, nil
	}
	if f.spoofURL != "" {
		spoofReq := req.Clone(req.Context())
		u, _ := url.Parse(f.spoofURL)
		spoofReq.URL = u
		return &http.Response{
			StatusCode: f.spoofStatus,
			Header:     http.Header{"Content-Type": []string{"application/json"}},
			Body:       ioNopCloserString(f.spoofBody),
			Request:    spoofReq,
		}, nil
	}
	return f.base.RoundTrip(req)
}

type stringReadCloser struct{ *strings.Reader }

func (stringReadCloser) Close() error { return nil }

func ioNopCloserString(s string) stringReadCloser { return stringReadCloser{strings.NewReader(s)} }

func TestCloudRequestOriginBindingAndSessionPersistence(t *testing.T) {
	var evilAuthHeaders atomic.Int32
	var evilHits atomic.Int32
	evilRelay := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		evilHits.Add(1)
		w.WriteHeader(http.StatusOK)
	}))
	defer evilRelay.Close()

	evilCloud := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		evilHits.Add(1)
		if r.Header.Get("Authorization") != "" || r.Header.Get("Cookie") != "" {
			evilAuthHeaders.Add(1)
		}
		if r.Method == http.MethodPost && r.URL.Path == "/api/v1/device-credentials/relay" {
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":                 "dc-spoof",
				"key_prefix":         "uc_dc_spoof",
				"credential":         "uc_dc_spoof_secret",
				"scopes":             []string{"relay"},
				"relay_url":          evilRelay.URL,
				"max_transfer_bytes": 25 << 20,
				"expires_at_ms":      time.Now().Add(time.Hour).UnixMilli(),
			})
			return
		}
		w.WriteHeader(http.StatusUnauthorized)
		_ = json.NewEncoder(w).Encode(map[string]string{"error": "invalid_grant"})
	}))
	defer evilCloud.Close()

	var revokeWith401 atomic.Bool
	var redirectAuth atomic.Bool
	primary := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if redirectAuth.Load() {
			http.Redirect(w, r, evilCloud.URL+r.URL.Path, http.StatusFound)
			return
		}
		if revokeWith401.Load() {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]string{"error": "invalid_grant"})
			return
		}
		switch {
		case r.URL.Path == "/api/auth/refresh":
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token":  "jwt-fresh",
				"refresh_token": "refresh-fresh",
				"expires_in":    900,
			})
		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodPost:
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":                 "dc-legit",
				"key_prefix":         "uc_dc_legit",
				"credential":         "uc_dc_legit_secret",
				"scopes":             []string{"relay"},
				"relay_url":          "",
				"max_transfer_bytes": 25 << 20,
				"expires_at_ms":      time.Now().Add(time.Hour).UnixMilli(),
				"rotate_after_ms":    time.Now().Add(30 * time.Minute).UnixMilli(),
			})
		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodDelete:
			w.WriteHeader(http.StatusNoContent)
		case r.URL.Path == "/api/auth/revoke":
			w.WriteHeader(http.StatusOK)
		default:
			http.NotFound(w, r)
		}
	}))
	defer primary.Close()

	// 1. Approved origin succeeds across issue/refresh/revoke and LoadSession/SaveSession reject invalid CloudURL.
	dir := t.TempDir()
	baseSess := Session{
		State:             SessionStateActive,
		CloudURL:          primary.URL + "/",
		AccessToken:       "jwt-init",
		AccessExpiresAtMs: time.Now().Add(time.Hour).UnixMilli(),
		RefreshToken:      "refresh-init",
		User:              User{Email: "alice@example.com"},
		SignedInAtMs:      1764200000000,
	}
	if err := SaveSession(dir, Session{State: SessionStateActive, CloudURL: "http://cloud.example.com"}); err == nil {
		t.Fatal("expected SaveSession to reject non-HTTPS non-loopback CloudURL")
	}
	if err := os.WriteFile(SessionPath(dir), []byte(`{"state":"active","cloud_url":"http://cloud.example.com","refresh_token":"r"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, ok, err := LoadSession(dir); err != nil || ok {
		t.Fatalf("expected tampered non-HTTPS non-loopback cloud_url rejected by LoadSession, got ok=%v err=%v", ok, err)
	}

	mgr := NewManager(dir, ManagerOptions{HTTPClient: primary.Client()})
	if _, err := mgr.SetSession(baseSess); err != nil {
		t.Fatalf("SetSession: %v", err)
	}

	// 2. HTTP redirect on /api/v1/device-credentials/relay must be rejected without hitting target or persisting response.
	redirectAuth.Store(true)
	if _, sessAfterRedir, err := mgr.EnsureDeviceCredential(context.Background()); err == nil {
		t.Fatal("expected redirect during EnsureDeviceCredential to fail")
	} else if isAuthoritativeRevocation(err) || sessAfterRedir.EffectiveState() == SessionStateReauthRequired {
		t.Fatalf("redirect must not produce authoritative revocation: err=%v state=%q", err, sessAfterRedir.EffectiveState())
	}
	loaded, ok, err := LoadSession(dir)
	if err != nil || !ok || loaded.EffectiveState() != SessionStateActive || loaded.DeviceCredential != nil {
		t.Fatalf("expected session active without persisted redirected credential, got ok=%v err=%v sess=%+v", ok, err, loaded)
	}

	// 3. Mismatched resp.Request.URL returning 401 invalid_grant must NOT revoke session; approved 401 invalid_grant MUST.
	spoofCli := &http.Client{
		Transport: &fakeCloudOriginRoundTripper{
			spoofURL:    evilCloud.URL + "/api/v1/device-credentials/relay",
			spoofStatus: http.StatusUnauthorized,
			spoofBody:   `{"error":"invalid_grant"}`,
		},
	}
	spoofMgr := NewManager(dir, ManagerOptions{HTTPClient: spoofCli})
	if _, sessAfterSpoof, err := spoofMgr.EnsureDeviceCredential(context.Background()); err == nil {
		t.Fatal("expected spoofed origin 401 to fail")
	} else if isAuthoritativeRevocation(err) || sessAfterSpoof.EffectiveState() == SessionStateReauthRequired {
		t.Fatalf("off-origin 401 must NOT trigger reauth_required: err=%v state=%q", err, sessAfterSpoof.EffectiveState())
	}
	// Verify Refresh, exchangeCode, deleteDeviceCredential, Logout reject mismatched response origin too.
	spoof200Cli := &http.Client{
		Transport: &fakeCloudOriginRoundTripper{
			spoofURL:    evilCloud.URL + "/api/v1/device-credentials/relay",
			spoofStatus: http.StatusCreated,
			spoofBody:   `{"id":"dc-x","key_prefix":"x","credential":"secret","relay_url":"https://relay.example.com"}`,
		},
	}
	if _, err := Refresh(context.Background(), spoof200Cli, baseSess); err == nil {
		t.Fatal("expected Refresh to reject mismatched response origin")
	}
	if _, err := exchangeCode(context.Background(), spoof200Cli, primary.URL, "code", "verifier", "http://127.0.0.1/cb", "dev"); err == nil {
		t.Fatal("expected exchangeCode to reject mismatched response origin")
	}
	if _, err := deleteDeviceCredential(context.Background(), spoof200Cli, primary.URL, "jwt"); err == nil {
		t.Fatal("expected deleteDeviceCredential to reject mismatched response origin")
	}
	if err := Logout(context.Background(), spoof200Cli, baseSess); err == nil {
		t.Fatal("expected Logout to reject mismatched response origin")
	}
	if evilHits.Load() != 0 || evilAuthHeaders.Load() != 0 {
		t.Fatalf("expected 0 requests/auth headers forwarded to redirected/spoofed target, hits=%d auth=%d", evilHits.Load(), evilAuthHeaders.Load())
	}

	// Normal approved flow recovers and legitimate 401 invalid_grant still revokes.
	redirectAuth.Store(false)
	cred, sessLegit, err := mgr.EnsureDeviceCredential(context.Background())
	if err != nil || cred.ID != "dc-legit" || sessLegit.EffectiveState() != SessionStateActive {
		t.Fatalf("expected normal approved EnsureDeviceCredential to succeed, got cred=%+v err=%v", cred, err)
	}
	revokeWith401.Store(true)
	if _, _, err := mgr.HandleRelayUnauthorized(context.Background(), cred.Credential); err == nil {
		t.Fatal("expected approved 401 invalid_grant to fail")
	}
	revokedSess, ok, err := LoadSession(dir)
	if err != nil || !ok || revokedSess.EffectiveState() != SessionStateReauthRequired {
		t.Fatalf("expected approved 401 invalid_grant to transition to reauth_required, got ok=%v err=%v state=%q", ok, err, revokedSess.EffectiveState())
	}
}
