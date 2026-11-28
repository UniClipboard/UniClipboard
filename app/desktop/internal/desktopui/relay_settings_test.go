package desktopui

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func assertAbsentTexts(t *testing.T, texts string, forbidden ...string) {
	t.Helper()
	for _, bad := range forbidden {
		if strings.Contains(texts, bad) {
			t.Fatalf("unexpected text %q rendered in UI: %s", bad, texts)
		}
	}
}

func assertNoAccessKeyRow(t *testing.T, nodeTexts []string) {
	t.Helper()
	if slices.Contains(nodeTexts, "接入密钥") {
		t.Fatalf("unexpected access-key row rendered in UI: %v", nodeTexts)
	}
	assertAbsentTexts(t, strings.Join(nodeTexts, " | "),
		"填写密钥", "更换", "清除", "重新获取密钥", "一键开启", "用于自建中转服务鉴权（仅写入不回显）。",
	)
}

func assertPresentTexts(t *testing.T, texts string, wants ...string) {
	t.Helper()
	for _, want := range wants {
		if !strings.Contains(texts, want) {
			t.Fatalf("expected %q in UI, got: %s", want, texts)
		}
	}
}

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
	assertPresentTexts(t, texts,
		"混合中转", "1/2 台已就绪", "启用混合中转", "服务地址", "单次大小上限",
		"接入密钥", "用于自建中转服务鉴权（仅写入不回显）。", "已配置", "更换", "清除", "恢复默认",
	)
	assertAbsentTexts(t, texts, "云服务账号", "退出登录", "重新获取密钥", "一键开启")

	// Clicking 恢复默认 switches SelfHost -> Official, clears custom URL and
	// AccessKeySet, and disables relay when signed out without env fallback.
	if err := tester.Click("恢复默认"); err != nil {
		t.Fatalf("click 恢复默认: %v", err)
	}
	tester.Frame()
	if !changed || cfg.Sync.Relay.URL != "" || cfg.Sync.Relay.AccessKeySet || cfg.Sync.Relay.Enabled {
		t.Fatalf("expected official reset (url=%q set=%v enabled=%v changed=%v)", cfg.Sync.Relay.URL, cfg.Sync.Relay.AccessKeySet, cfg.Sync.Relay.Enabled, changed)
	}

	officialTexts := strings.Join(tester.Texts(), " | ")
	assertPresentTexts(t, officialTexts, "云服务账号", "官方服务需登录账号后使用。", "登录", "服务地址", "保存")
	assertAbsentTexts(t, officialTexts, "接入密钥", "填写密钥", "更换", "清除", "重新获取密钥", "一键开启")

	// Service URL input remains discoverable and saveable while signed out in official mode.
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

	d.relay.urlDraft = "https://RELAY.Home.Example:443/"
	tester.Frame()
	if err := tester.Click("保存"); err != nil {
		t.Fatalf("click 保存 valid URL: %v", err)
	}
	tester.Frame()
	if !changed || d.relay.urlErr != "" || cfg.Sync.Relay.URL != "https://relay.home.example" {
		t.Fatalf("expected normalized URL saved, got err=%q url=%q changed=%v", d.relay.urlErr, cfg.Sync.Relay.URL, changed)
	}
	assertPresentTexts(t, strings.Join(tester.Texts(), " | "), "接入密钥", "填写密钥")
}

func TestRelaySettingsOfficialStatesToggleAndStability(t *testing.T) {
	dir := t.TempDir()
	cfg := config.Default()
	cfg.DeviceName = "Desk Mac"
	cfg.Sync.Relay.URL = ""
	cfg.Sync.Relay.Enabled = false

	provCalls := 0
	cloudSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodPost {
			provCalls++
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":                 "dc-2",
				"key_prefix":         "uc_dc_synth0002",
				"credential":         "uc_dc_synth0002secretvalue",
				"scopes":             []string{"relay"},
				"relay_url":          "https://relay.uniclipboard.app",
				"max_transfer_bytes": 25 << 20,
				"expires_at_ms":      time.Now().Add(24 * time.Hour).UnixMilli(),
				"rotate_after_ms":    time.Now().Add(12 * time.Hour).UnixMilli(),
			})
			return
		}
		http.NotFound(w, r)
	}))
	defer cloudSrv.Close()

	activeSess := cloudauth.Session{
		State:        cloudauth.SessionStateActive,
		CloudURL:     cloudSrv.URL,
		AccessToken:  "jwt-test",
		RefreshToken: "refresh-test",
		User:         cloudauth.User{Email: "alice@example.com"},
		SignedInAtMs: 1764200000000,
	}
	if err := cloudauth.SaveSession(dir, activeSess); err != nil {
		t.Fatalf("SaveSession: %v", err)
	}

	loginCalls := 0
	d := &viewState{
		language: "zh-CN",
		options:  map[string]bool{},
		choices:  map[string]string{},
		live: &liveState{
			ctx:     context.Background(),
			loaded:  true,
			results: make(chan func(), 8),
			config:  config.State{Revision: "rev-1", Path: filepath.Join(dir, "config.toml"), Config: cfg},
		},
		cloud: cloudAccountState{
			httpClient: cloudSrv.Client(),
			loginFn: func(_ context.Context, _ cloudauth.LoginOptions) (cloudauth.Session, error) {
				loginCalls++
				return activeSess, nil
			},
		},
	}

	k := desktopkit.Kit{}
	changed := false
	tester := ui.NewTester(func(c *ui.Context) {
		k.Apply(c)
		ui.Column(c).Children(func() {
			d.relaySettingsGroup(c, k, &cfg, &changed)
			d.cloudAccountGroup(c, k, &cfg)
		})
	}, 960, 820)
	tester.Frame()

	texts := strings.Join(tester.Texts(), " | ")
	assertPresentTexts(t, texts, "已登录 alice@example.com", "已登录", "退出登录")
	assertAbsentTexts(t, texts, "接入密钥", "更换", "清除", "重新获取密钥", "一键开启", "uc_dc_", "secretvalue")

	clickSwitch := func(label string) {
		t.Helper()
		n, ok := tester.AccessByRole(ui.RoleSwitch, label)
		if !ok {
			t.Fatalf("missing switch %q", label)
		}
		tester.ClickAt(n.Bounds.X+n.Bounds.W/2, n.Bounds.Y+n.Bounds.H/2)
	}

	// Toggling ON while signed in without active credential triggers provisioning.
	clickSwitch("启用混合中转")
	tester.Frame()
	if !d.cloud.busy || loginCalls != 0 {
		t.Fatalf("expected direct provisioning without login dialog, busy=%v loginCalls=%d", d.cloud.busy, loginCalls)
	}
	var fn func()
	select {
	case fn = <-d.live.results:
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for relay provisioning")
	}
	d.live.busy = true // prevent saveLiveConfig from calling nil client in unit test; real IPC tested in settings-cloud-relay
	fn()
	d.live.busy = false
	d.live.busySince = time.Time{}
	loaded, ok, err := cloudauth.LoadSession(dir)
	if provCalls != 1 || err != nil || !ok || loaded.DeviceCredential == nil || loaded.DeviceCredential.ID != "dc-2" {
		t.Fatalf("expected 1 provision call and persisted dc-2, provCalls=%d ok=%v err=%v cred=%+v", provCalls, ok, err, loaded.DeviceCredential)
	}
	cfg.Sync.Relay.Enabled = true
	d.live.config.Config.Sync.Relay.Enabled = true
	tester.Frame()
	assertAbsentTexts(t, strings.Join(tester.Texts(), " | "), "uc_dc_", "secretvalue")

	// Fast busy (<120ms) keeps controls visually enabled while ignoring concurrent clicks.
	d.live.busy = true
	d.live.busySince = time.Now()
	tester.Frame()
	sw, ok := tester.AccessByRole(ui.RoleSwitch, "启用混合中转")
	if !ok || sw.Disabled {
		t.Fatalf("expected relay switch visually enabled under <120ms busy threshold, ok=%v disabled=%v", ok, sw.Disabled)
	}
	clickSwitch("启用混合中转")
	if !cfg.Sync.Relay.Enabled {
		t.Fatal("click under busy must be ignored")
	}
	d.live.busy = false
	d.live.busySince = time.Time{}

	// External update to reauth_required is detected cleanly on refresh.
	reauthSess := activeSess
	reauthSess.State = cloudauth.SessionStateReauthRequired
	if err := cloudauth.SaveSession(dir, reauthSess); err != nil {
		t.Fatalf("SaveSession reauth: %v", err)
	}
	// Ensure mtime differs even on coarse filesystem timestamps.
	future := time.Now().Add(2 * time.Second)
	_ = os.Chtimes(cloudauth.SessionPath(dir), future, future)
	d.cloud.lastStatAt = time.Time{}
	tester.Frame()
	reauthTexts := strings.Join(tester.Texts(), " | ")
	assertPresentTexts(t, reauthTexts, "需重新登录", "重新登录", "登录已失效，请重新登录。", "登录已失效，请重新登录以恢复官方服务。")
	assertAbsentTexts(t, reauthTexts, "接入密钥", "更换", "清除", "uc_dc_")

	// Clicking toggle ON while reauth_required without fallback launches login.
	cfg.Sync.Relay.Enabled = false
	tester.Frame()
	clickSwitch("启用混合中转")
	tester.Frame()
	if !d.cloud.busy {
		t.Fatal("expected startCloudLogin when enabling official relay in reauth_required state")
	}
	select {
	case fn = <-d.live.results:
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for reauth login result")
	}
	d.live.busy = true
	fn()
	d.live.busy = false
	if loginCalls != 1 || !d.cloud.signedIn || d.cloud.reauthRequired {
		t.Fatalf("expected reauth recovery, loginCalls=%d cloud=%+v", loginCalls, d.cloud)
	}

	// Signed-out with host-reported env fallback in official mode hides access key row,
	// notes manual fallback, and allows toggling relay directly; desktop-only env vars
	// must NOT influence hasRelayFallbackKey.
	if err := os.Remove(cloudauth.SessionPath(dir)); err != nil {
		t.Fatalf("remove cloud-session.json: %v", err)
	}
	d.cloud.lastStatAt = time.Time{}
	t.Setenv("UNIC_RELAY_ACCESS_KEY", "desktop-only-must-be-ignored")
	cfg = d.live.config.Config
	cfg.Sync.Relay.Enabled = false
	cfg.Sync.Relay.AccessKeySet = false
	cfg.Sync.Relay.EnvAccessKeySet = false
	changed = false
	tester.Frame()
	assertAbsentTexts(t, strings.Join(tester.Texts(), " | "), "当前使用手动配置的接入密钥，也可登录云账号。")

	cfg.Sync.Relay.EnvAccessKeySet = true
	tester.Frame()
	fallbackNodes := tester.Texts()
	fallbackTexts := strings.Join(fallbackNodes, " | ")
	assertPresentTexts(t, fallbackTexts, "当前使用手动配置的接入密钥，也可登录云账号。", "登录")
	assertNoAccessKeyRow(t, fallbackNodes)
	assertAbsentTexts(t, fallbackTexts, "desktop-only-must-be-ignored")
	clickSwitch("启用混合中转")
	tester.Frame()
	if !changed || !cfg.Sync.Relay.Enabled || d.cloud.busy {
		t.Fatalf("expected direct toggle with host env fallback, changed=%v enabled=%v busy=%v", changed, cfg.Sync.Relay.Enabled, d.cloud.busy)
	}

	// Non-default Cloud-advertised RelayURL keeps sync.relay.url empty in config.
	d.live.busy = true
	d.live.config.Config.Sync.Relay.URL = ""
	d.applyCloudRelayCredential(cloudauth.DeviceCredential{ID: "dc-nondefault", RelayURL: "http://127.0.0.1:45678"})
	if d.relay.urlSynced != "" || d.relay.urlDraft != "" {
		t.Fatalf("expected empty official url drafts after non-default cloud relay_url, got (%q, %q)", d.relay.urlDraft, d.relay.urlSynced)
	}
	d.live.busy = false

	// Cloud busy <120ms keeps controls visually enabled; >=120ms disables them.
	d.cloud.busy = true
	d.cloud.busySince = time.Now()
	d.cloud.busyLabel = d.tr("等待浏览器登录…")
	tester.Frame()
	if d.cloud.showBusy() {
		t.Fatal("expected d.cloud.showBusy()=false inside 120ms window")
	}
	swFast, ok := tester.AccessByRole(ui.RoleSwitch, "启用混合中转")
	if !ok || swFast.Disabled {
		t.Fatalf("expected relay switch visually enabled under <120ms cloud busy, ok=%v disabled=%v", ok, swFast.Disabled)
	}
	d.cloud.busySince = time.Now().Add(-200 * time.Millisecond)
	tester.Frame()
	swSlow, ok := tester.AccessByRole(ui.RoleSwitch, "启用混合中转")
	if !ok || !swSlow.Disabled {
		t.Fatalf("expected relay switch visually disabled after >=120ms cloud busy, ok=%v disabled=%v", ok, swSlow.Disabled)
	}
	d.cloud.busy = false
	d.cloud.busySince = time.Time{}
}

func TestCloudAccountLoginPersistsSessionAndProvisionsRelay(t *testing.T) {
	dir := t.TempDir()
	cfg := config.Default()
	cfg.DeviceName = "Work Mac"

	cloudSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodPost {
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":                 "dc-1",
				"key_prefix":         "uc_dc_test1234",
				"credential":         "uc_dc_test1234secretvalue",
				"scopes":             []string{"relay"},
				"relay_url":          "https://relay.uniclipboard.app",
				"max_transfer_bytes": 25 << 20,
				"expires_at_ms":      time.Now().Add(24 * time.Hour).UnixMilli(),
				"rotate_after_ms":    time.Now().Add(12 * time.Hour).UnixMilli(),
			})
			return
		}
		http.NotFound(w, r)
	}))
	defer cloudSrv.Close()

	d := &viewState{
		language: "zh-CN",
		options:  map[string]bool{},
		choices:  map[string]string{},
		live: &liveState{
			ctx:     context.Background(),
			loaded:  true,
			results: make(chan func(), 8),
			config:  config.State{Revision: "rev-1", Path: filepath.Join(dir, "config.toml"), Config: cfg},
		},
		cloud: cloudAccountState{
			httpClient: cloudSrv.Client(),
			loginFn: func(_ context.Context, opts cloudauth.LoginOptions) (cloudauth.Session, error) {
				if opts.DeviceName != "Work Mac" {
					t.Errorf("DeviceName = %q, want Work Mac", opts.DeviceName)
				}
				return cloudauth.Session{
					State:        cloudauth.SessionStateActive,
					CloudURL:     cloudSrv.URL,
					AccessToken:  "jwt-test",
					RefreshToken: "refresh-test",
					User:         cloudauth.User{Email: "alice@example.com"},
					SignedInAtMs: 1764200000000,
				}, nil
			},
		},
	}

	k := desktopkit.Kit{}
	tester := ui.NewTester(func(c *ui.Context) {
		k.Apply(c)
		ui.Column(c).Children(func() {
			d.cloudAccountGroup(c, k, &cfg)
		})
	}, 900, 700)
	tester.Frame()

	if err := tester.Click("登录"); err != nil {
		t.Fatalf("click cloud login: %v", err)
	}
	tester.Frame()
	if !d.cloud.busy {
		t.Fatal("expected d.cloud.busy=true while waiting for browser sign-in")
	}

	var fn func()
	select {
	case fn = <-d.live.results:
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for cloud login result")
	}
	d.live.busy = true // prevent mutate from calling nil client
	fn()
	if d.cloud.busy || d.cloud.err != "" || !d.cloud.signedIn || d.cloud.session.User.Email != "alice@example.com" {
		t.Fatalf("unexpected cloud state after login: %+v", d.cloud)
	}
	loaded, ok, err := cloudauth.LoadSession(dir)
	if err != nil || !ok || loaded.User.Email != "alice@example.com" || loaded.DeviceCredential == nil || loaded.DeviceCredential.ID != "dc-1" {
		t.Fatalf("expected persisted cloud session with device credential, got (%+v, %v, %v)", loaded, ok, err)
	}
}
