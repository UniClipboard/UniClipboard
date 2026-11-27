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

func TestLoginForRelayKeyEndToEnd(t *testing.T) {
	var mu sync.Mutex
	var savedChallenge, savedRedirect string

	cloud := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/cli/authorize":
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

		case "/api/auth/token":
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
				"access_token": "jwt-access-token-xyz",
				"token_type":   "Bearer",
				"expires_in":   900,
			})

		case "/api/relay/v2/api-keys":
			if r.Header.Get("Authorization") != "Bearer jwt-access-token-xyz" {
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
		default:
			http.NotFound(w, r)
		}
	}))
	defer cloud.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	cred, err := LoginForRelayKey(ctx, LoginOptions{
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
		t.Fatalf("LoginForRelayKey: %v", err)
	}
	if cred.APIKey != "uc_rk_1234567890abcdef" || cred.KeyPrefix != "uc_rk_12345678" || cred.RelayURL != "https://relay.uniclipboard.app" {
		t.Fatalf("unexpected cred: %+v", cred)
	}
}

func TestLoginForRelayKeyStateMismatch(t *testing.T) {
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

	_, err := LoginForRelayKey(ctx, LoginOptions{
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
