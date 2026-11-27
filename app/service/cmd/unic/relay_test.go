package main

import (
	"bytes"
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestRunRelayLoginSavesKeyWithoutEchoingSecret(t *testing.T) {
	home := t.TempDir()
	t.Setenv("UNIC_HOME", home)

	var stdout, stderr bytes.Buffer
	secretKey := "uc_rk_super_secret_key_never_print"
	err := runRelayLogin(
		context.Background(),
		[]string{"--json"},
		&stdout,
		&stderr,
		false,
		func(_ context.Context, _ cloudauth.LoginOptions) (cloudauth.RelayCredential, error) {
			return cloudauth.RelayCredential{
				KeyID:            "key-1",
				KeyPrefix:        "uc_rk_super_se",
				APIKey:           secretKey,
				RelayURL:         "https://relay.uniclipboard.app",
				MaxTransferBytes: 25 << 20,
			}, nil
		},
	)
	if err != nil {
		t.Fatalf("runRelayLogin: %v", err)
	}
	if strings.Contains(stdout.String(), secretKey) || strings.Contains(stderr.String(), secretKey) {
		t.Fatalf("raw API key leaked to stdout/stderr: out=%q err=%q", stdout.String(), stderr.String())
	}
	var out struct {
		Authorized bool   `json:"authorized"`
		Enabled    bool   `json:"enabled"`
		RelayURL   string `json:"relay_url"`
		KeyPrefix  string `json:"key_prefix"`
	}
	if err := json.Unmarshal(stdout.Bytes(), &out); err != nil {
		t.Fatalf("unmarshal stdout: %v (%s)", err, stdout.String())
	}
	if !out.Authorized || !out.Enabled || out.RelayURL != "https://relay.uniclipboard.app" || out.KeyPrefix != "uc_rk_super_se" {
		t.Fatalf("unexpected json output: %+v", out)
	}

	mgr, err := config.Open(filepath.Join(home, "config.toml"), nil)
	if err != nil {
		t.Fatalf("reopen config.toml: %v", err)
	}
	cur := mgr.Current()
	if !cur.Sync.Relay.Enabled || cur.Sync.Relay.URL != "" || cur.Sync.Relay.AccessKey != secretKey {
		t.Fatalf("saved config = %+v", cur.Sync.Relay)
	}
}
