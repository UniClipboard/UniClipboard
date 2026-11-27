package main

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"path/filepath"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestRunCloudLoginStatusAndLogout(t *testing.T) {
	home := t.TempDir()
	t.Setenv("UNIC_HOME", home)

	secretRefresh := "opaque-refresh-never-print"
	secretKey := "uc_rk_super_secret_key_never_print"
	revokedKey := ""
	revokedSession := false

	deps := cloudCmdDeps{
		loginFn: func(_ context.Context, _ cloudauth.LoginOptions) (cloudauth.Session, error) {
			return cloudauth.Session{
				CloudURL:     "https://cloud.uniclipboard.app",
				AccessToken:  "jwt-never-print",
				RefreshToken: secretRefresh,
				User:         cloudauth.User{Email: "alice@example.com"},
				SignedInAtMs: 1764200000000,
			}, nil
		},
		createRelayFn: func(_ context.Context, _ *http.Client, sess cloudauth.Session, _ string) (cloudauth.RelayCredential, cloudauth.Session, error) {
			sess.RelayKeyID = "key-1"
			sess.RelayKeyPrefix = "uc_rk_super_se"
			return cloudauth.RelayCredential{
				KeyID:            "key-1",
				KeyPrefix:        "uc_rk_super_se",
				APIKey:           secretKey,
				RelayURL:         "https://relay.uniclipboard.app",
				MaxTransferBytes: 25 << 20,
			}, sess, nil
		},
		revokeRelayFn: func(_ context.Context, _ *http.Client, _ cloudauth.Session, keyID string) error {
			revokedKey = keyID
			return nil
		},
		logoutFn: func(_ context.Context, _ *http.Client, _ cloudauth.Session) error {
			revokedSession = true
			return nil
		},
	}

	var stdout, stderr bytes.Buffer
	if err := runCloudLogin(context.Background(), []string{"--json"}, &stdout, &stderr, false, deps); err != nil {
		t.Fatalf("runCloudLogin: %v", err)
	}
	for _, secret := range []string{secretKey, secretRefresh, "jwt-never-print"} {
		if strings.Contains(stdout.String(), secret) || strings.Contains(stderr.String(), secret) {
			t.Fatalf("secret %q leaked to output: out=%q err=%q", secret, stdout.String(), stderr.String())
		}
	}
	var loginOut struct {
		SignedIn       bool   `json:"signed_in"`
		Account        string `json:"account"`
		RelayEnabled   bool   `json:"relay_enabled"`
		RelayKeyPrefix string `json:"relay_key_prefix"`
	}
	if err := json.Unmarshal(stdout.Bytes(), &loginOut); err != nil {
		t.Fatalf("unmarshal login stdout: %v", err)
	}
	if !loginOut.SignedIn || loginOut.Account != "alice@example.com" || !loginOut.RelayEnabled || loginOut.RelayKeyPrefix != "uc_rk_super_se" {
		t.Fatalf("unexpected login output: %+v", loginOut)
	}

	mgr, err := config.Open(filepath.Join(home, "config.toml"), nil)
	if err != nil {
		t.Fatalf("reopen config.toml: %v", err)
	}
	if cur := mgr.Current().Sync.Relay; !cur.Enabled || cur.URL != "" || cur.AccessKey != secretKey {
		t.Fatalf("saved relay config = %+v", cur)
	}

	// Status reports signed-in account without leaking secrets.
	var statusOut bytes.Buffer
	if err := runCloudStatus([]string{"--json"}, &statusOut, false); err != nil {
		t.Fatalf("runCloudStatus: %v", err)
	}
	if !strings.Contains(statusOut.String(), "alice@example.com") || strings.Contains(statusOut.String(), secretRefresh) {
		t.Fatalf("unexpected status output: %s", statusOut.String())
	}

	// Logout revokes Cloud session + Relay key and clears official relay config.
	var logoutOut bytes.Buffer
	if err := runCloudLogout(context.Background(), []string{"--json"}, &logoutOut, false, deps); err != nil {
		t.Fatalf("runCloudLogout: %v", err)
	}
	if !revokedSession || revokedKey != "key-1" {
		t.Fatalf("expected revoke on logout, got session=%v key=%q", revokedSession, revokedKey)
	}
	if _, ok, err := cloudauth.LoadSession(home); err != nil || ok {
		t.Fatalf("expected session deleted, got ok=%v err=%v", ok, err)
	}
	mgr2, err := config.Open(filepath.Join(home, "config.toml"), nil)
	if err != nil {
		t.Fatalf("reopen config after logout: %v", err)
	}
	if cur := mgr2.Current().Sync.Relay; cur.Enabled || cur.AccessKey != "" {
		t.Fatalf("expected official relay cleared on logout, got %+v", cur)
	}
}
