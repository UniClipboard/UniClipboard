package main

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestRunCloudLoginStatusAndLogout(t *testing.T) {
	home := t.TempDir()
	t.Setenv("UNIC_HOME", home)

	secretRefresh := "opaque-refresh-never-print"
	secretCred := "uc_dc_super_secret_never_print"
	var mu sync.Mutex
	revokedCred := false
	revokedSession := false

	cloudSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodPost:
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":                 "dc-1",
				"key_prefix":         "uc_dc_super_se",
				"credential":         secretCred,
				"scopes":             []string{"relay"},
				"relay_url":          "https://relay.uniclipboard.app",
				"max_transfer_bytes": 25 << 20,
				"expires_at_ms":      time.Now().Add(24 * time.Hour).UnixMilli(),
				"rotate_after_ms":    time.Now().Add(12 * time.Hour).UnixMilli(),
			})
		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodDelete:
			mu.Lock()
			revokedCred = true
			mu.Unlock()
			w.WriteHeader(http.StatusNoContent)
		case r.URL.Path == "/api/auth/revoke" && r.Method == http.MethodPost:
			mu.Lock()
			revokedSession = true
			mu.Unlock()
			w.WriteHeader(http.StatusNoContent)
		default:
			http.NotFound(w, r)
		}
	}))
	defer cloudSrv.Close()

	deps := cloudCmdDeps{
		httpClient: cloudSrv.Client(),
		loginFn: func(_ context.Context, _ cloudauth.LoginOptions) (cloudauth.Session, error) {
			return cloudauth.Session{
				State:        cloudauth.SessionStateActive,
				CloudURL:     cloudSrv.URL,
				AccessToken:  "jwt-never-print",
				RefreshToken: secretRefresh,
				User:         cloudauth.User{Email: "alice@example.com"},
				SignedInAtMs: 1764200000000,
			}, nil
		},
	}

	var stdout, stderr bytes.Buffer
	if err := runCloudLogin(context.Background(), []string{"--json"}, &stdout, &stderr, false, deps); err != nil {
		t.Fatalf("runCloudLogin: %v", err)
	}
	for _, secret := range []string{secretCred, secretRefresh, "jwt-never-print"} {
		if strings.Contains(stdout.String(), secret) || strings.Contains(stderr.String(), secret) {
			t.Fatalf("secret %q leaked to output: out=%q err=%q", secret, stdout.String(), stderr.String())
		}
	}
	var loginOut struct {
		SignedIn         bool   `json:"signed_in"`
		State            string `json:"state"`
		Account          string `json:"account"`
		RelayEnabled     bool   `json:"relay_enabled"`
		CredentialPrefix string `json:"credential_prefix"`
	}
	if err := json.Unmarshal(stdout.Bytes(), &loginOut); err != nil {
		t.Fatalf("unmarshal login stdout: %v", err)
	}
	if !loginOut.SignedIn || loginOut.State != cloudauth.SessionStateActive || loginOut.Account != "alice@example.com" || !loginOut.RelayEnabled || loginOut.CredentialPrefix != "uc_dc_super_se" {
		t.Fatalf("unexpected login output: %+v", loginOut)
	}

	// config.toml must enable official relay without writing any access_key secret.
	mgr, err := config.Open(filepath.Join(home, "config.toml"), nil)
	if err != nil {
		t.Fatalf("reopen config.toml: %v", err)
	}
	if cur := mgr.Current().Sync.Relay; !cur.Enabled || cur.URL != "" || cur.AccessKey != "" {
		t.Fatalf("saved relay config = %+v (expected enabled=true, url=\"\", access_key=\"\")", cur)
	}
	if rawCfg, err := os.ReadFile(filepath.Join(home, "config.toml")); err != nil || strings.Contains(string(rawCfg), secretCred) {
		t.Fatalf("config.toml leaked device credential or could not read: %v", err)
	}

	// Status reports signed-in account without leaking secrets.
	var statusOut bytes.Buffer
	if err := runCloudStatus([]string{"--json"}, &statusOut, false); err != nil {
		t.Fatalf("runCloudStatus: %v", err)
	}
	for _, secret := range []string{secretCred, secretRefresh, "jwt-never-print"} {
		if strings.Contains(statusOut.String(), secret) {
			t.Fatalf("secret %q leaked to status output: %s", secret, statusOut.String())
		}
	}
	if !strings.Contains(statusOut.String(), "alice@example.com") || !strings.Contains(statusOut.String(), "uc_dc_super_se") {
		t.Fatalf("unexpected status output: %s", statusOut.String())
	}

	// Logout revokes Cloud session + device credential and disables official relay.
	var logoutOut bytes.Buffer
	if err := runCloudLogout(context.Background(), []string{"--json"}, &logoutOut, false, deps); err != nil {
		t.Fatalf("runCloudLogout: %v", err)
	}
	mu.Lock()
	gotRevCred, gotRevSess := revokedCred, revokedSession
	mu.Unlock()
	if !gotRevSess || !gotRevCred {
		t.Fatalf("expected revoke on logout, got session=%v cred=%v", gotRevSess, gotRevCred)
	}
	if _, ok, err := cloudauth.LoadSession(home); err != nil || ok {
		t.Fatalf("expected session deleted, got ok=%v err=%v", ok, err)
	}
	mgr2, err := config.Open(filepath.Join(home, "config.toml"), nil)
	if err != nil {
		t.Fatalf("reopen config after logout: %v", err)
	}
	if cur := mgr2.Current().Sync.Relay; cur.Enabled || cur.AccessKey != "" {
		t.Fatalf("expected official relay disabled on logout, got %+v", cur)
	}
}
