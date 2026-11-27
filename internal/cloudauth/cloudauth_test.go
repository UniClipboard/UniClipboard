package cloudauth

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
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
	var revokedRefresh, revokedKeyID string

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
				http.Error(w, "bad refresh", http.StatusUnauthorized)
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token":  "jwt-access-2",
				"refresh_token": "opaque-refresh-2",
				"expires_in":    900,
			})

		case r.URL.Path == "/api/relay/v2/api-keys" && r.Method == http.MethodPost:
			if r.Header.Get("Authorization") != "Bearer jwt-access-2" {
				http.Error(w, "unauthorized", http.StatusUnauthorized)
				return
			}
			var body struct {
				Name string `json:"name"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil || body.Name != "My MacBook" {
				http.Error(w, "bad key req", http.StatusBadRequest)
				return
			}
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":                 "key-uuid-1",
				"name":               body.Name,
				"key_prefix":         "uc_rk_12345678",
				"api_key":            "uc_rk_1234567890abcdef",
				"relay_url":          "https://relay.uniclipboard.app",
				"max_transfer_bytes": 26214400,
			})

		case strings.HasPrefix(r.URL.Path, "/api/relay/v2/api-keys/") && r.Method == http.MethodDelete:
			mu.Lock()
			revokedKeyID = strings.TrimPrefix(r.URL.Path, "/api/relay/v2/api-keys/")
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

	// Simulate expired access token -> CreateRelayAPIKey must auto-refresh and rotate.
	sess.AccessExpiresAtMs = time.Now().Add(-time.Minute).UnixMilli()
	cred, updated, err := CreateRelayAPIKey(ctx, cloud.Client(), sess, "")
	if err != nil {
		t.Fatalf("CreateRelayAPIKey: %v", err)
	}
	if cred.APIKey != "uc_rk_1234567890abcdef" || updated.RefreshToken != "opaque-refresh-2" || updated.RelayKeyID != "key-uuid-1" {
		t.Fatalf("unexpected cred/session: cred=%+v sess=%+v", cred, updated)
	}

	// Persist and reload session.
	dir := t.TempDir()
	if err := SaveSession(dir, updated); err != nil {
		t.Fatalf("SaveSession: %v", err)
	}
	loaded, ok, err := LoadSession(dir)
	if err != nil || !ok || loaded.User.DisplayLabel() != "alice@example.com" || loaded.RelayKeyPrefix != "uc_rk_12345678" {
		t.Fatalf("LoadSession = (%+v, %v, %v)", loaded, ok, err)
	}

	if err := RevokeRelayAPIKey(ctx, cloud.Client(), loaded, loaded.RelayKeyID); err != nil {
		t.Fatalf("RevokeRelayAPIKey: %v", err)
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
	if revokedKeyID != "key-uuid-1" || revokedRefresh != "opaque-refresh-2" {
		t.Fatalf("revocations = (key=%q, refresh=%q)", revokedKeyID, revokedRefresh)
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
