package desktopui

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestRelaySettingsGroupRenderAndInteractions(t *testing.T) {
	cfg := config.Default()
	cfg.Sync.Relay.Enabled = true
	cfg.Sync.Relay.URL = "https://relay.example.com"
	cfg.Sync.Relay.MaxBytes = 50 << 20
	cfg.Sync.Relay.AccessKeySet = true

	d := &viewState{
		language: "zh-CN",
		options:  map[string]bool{},
		choices:  map[string]string{},
		devices: []device{
			{
				id: "dev-1", name: "iPhone",
				bindings: []deviceBinding{{instanceID: string(engine.BackendTailcat), state: "authorized", relayRouteReady: true}},
			},
			{
				id: "dev-2", name: "iPad",
				bindings: []deviceBinding{{instanceID: string(engine.BackendTailcat), state: "authorized", relayRouteReady: false}},
			},
		},
		live: &liveState{
			ctx:     context.Background(),
			loaded:  true,
			results: make(chan func(), 8),
			config:  config.State{Config: cfg},
		},
	}

	k := desktopkit.Kit{}
	changed := false
	tester := ui.NewTester(func(c *ui.Context) {
		k.Apply(c)
		ui.Column(c).Children(func() {
			d.relaySettingsGroup(c, k, &cfg, &changed)
		})
	}, 900, 700)
	tester.Frame()

	texts := strings.Join(tester.Texts(), " | ")
	for _, want := range []string{"混合中转", "1/2 台设备中转就绪", "启用混合中转", "中转服务地址", "单次中转大小上限", "UniClipboard Cloud 授权", "中转 API Key / 接入密钥", "已配置", "清除密钥", "恢复默认"} {
		if !strings.Contains(texts, want) {
			t.Fatalf("expected %q in relay settings UI, got: %s", want, texts)
		}
	}

	// Clicking 恢复默认 clears custom URL and marks changed.
	if err := tester.Click("恢复默认"); err != nil {
		t.Fatalf("click 恢复默认: %v", err)
	}
	tester.Frame()
	if !changed || cfg.Sync.Relay.URL != "" {
		t.Fatalf("expected URL cleared and changed=true, got url=%q changed=%v", cfg.Sync.Relay.URL, changed)
	}

	// Submitting an invalid URL surfaces validation error without mutating config.
	changed = false
	d.relay.urlDraft = "ftp://bad.example"
	tester.Frame()
	if err := tester.Click("保存"); err != nil {
		t.Fatalf("click 保存: %v", err)
	}
	tester.Frame()
	if changed || d.relay.urlErr == "" || cfg.Sync.Relay.URL != "" {
		t.Fatalf("expected invalid URL rejected, got err=%q url=%q changed=%v", d.relay.urlErr, cfg.Sync.Relay.URL, changed)
	}

	// Valid URL normalizes and saves.
	d.relay.urlDraft = "https://RELAY.Home.Example:443/"
	tester.Frame()
	if err := tester.Click("保存"); err != nil {
		t.Fatalf("click 保存 valid URL: %v", err)
	}
	tester.Frame()
	if !changed || d.relay.urlErr != "" || cfg.Sync.Relay.URL != "https://relay.home.example" {
		t.Fatalf("expected normalized URL saved, got err=%q url=%q changed=%v", d.relay.urlErr, cfg.Sync.Relay.URL, changed)
	}
}

func TestRelayCloudLoginAutoFillsKeyAndEnablesRelay(t *testing.T) {
	cfg := config.Default()
	cfg.DeviceName = "Work Mac"

	d := &viewState{
		language: "zh-CN",
		options:  map[string]bool{},
		choices:  map[string]string{},
		live: &liveState{
			ctx:     context.Background(),
			loaded:  true,
			results: make(chan func(), 8),
			config:  config.State{Revision: "rev-1", Config: cfg},
		},
		relay: relaySettingsState{
			cloudLoginFn: func(_ context.Context, opts cloudauth.LoginOptions) (cloudauth.RelayCredential, error) {
				if opts.DeviceName != "Work Mac" {
					t.Errorf("DeviceName = %q, want Work Mac", opts.DeviceName)
				}
				return cloudauth.RelayCredential{
					KeyID:     "k1",
					KeyPrefix: "uc_rk_test1234",
					APIKey:    "uc_rk_test1234secretvalue",
					RelayURL:  "https://relay.uniclipboard.app",
				}, nil
			},
		},
	}

	k := desktopkit.Kit{}
	changed := false
	tester := ui.NewTester(func(c *ui.Context) {
		k.Apply(c)
		ui.Column(c).Children(func() {
			d.relaySettingsGroup(c, k, &cfg, &changed)
		})
	}, 900, 700)
	tester.Frame()

	if err := tester.Click("登录 Cloud 获取 API Key"); err != nil {
		t.Fatalf("click cloud login: %v", err)
	}
	tester.Frame()
	if !d.relay.cloudLoginBusy {
		t.Fatal("expected cloudLoginBusy=true while waiting for callback")
	}

	var fn func()
	select {
	case fn = <-d.live.results:
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for cloud login result")
	}
	d.live.busy = true // prevent mutate from using nil client while still checking state updates
	fn()
	if d.relay.cloudLoginBusy || d.relay.cloudLoginErr != "" {
		t.Fatalf("expected cloud login state cleared, got busy=%v err=%q", d.relay.cloudLoginBusy, d.relay.cloudLoginErr)
	}
}
