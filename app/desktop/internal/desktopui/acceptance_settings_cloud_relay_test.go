package desktopui

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"image/png"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
	"github.com/uniclipboard/unic/internal/cloudauth"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type relayTraceEntry struct {
	Target   string `json:"target"`
	Kind     string `json:"kind"` // "control" or "sse"
	CredKind string `json:"cred_kind"`
	AtMs     int64  `json:"at_ms"`
}

// scenarioSettingsCloudRelay exercises Settings > Cloud and Settings > Sync > Hybrid Relay
// across production desktopui, isolated real unicd processes, controlled TLS Cloud/Relay
// fixtures with test-only SSL_CERT_FILE trust, and BoardUI software rendering.
func scenarioSettingsCloudRelay(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("/tmp", "urelayui-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if e := os.RemoveAll(private); e != nil {
			t.Error(e)
		}
	}()
	out := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS")
	if out == "" {
		out = filepath.Join(private, "artifacts")
	}
	if err = os.MkdirAll(out, 0700); err != nil {
		t.Fatal(err)
	}

	checks := []string{}
	views := []map[string]any{}
	expirySummary := map[string]any{}
	receipt := map[string]any{
		"scope":                       "production desktopui + isolated real unicd processes + controlled TLS cloud/relay; software view",
		"go":                          runtime.Version(),
		"os":                          runtime.GOOS,
		"source_head":                 os.Getenv("UNIC_DESKTOP_E2E_SOURCE_HEAD"),
		"native_ui":                   false,
		"renderer":                    "software",
		"system_clipboard":            false,
		"native_window":               "NOT_RUN",
		"native_accessibility_bridge": "NOT_RUN",
		"live_clerk_browser":          "NOT_RUN",
	}
	hash := func(b []byte) string { v := sha256.Sum256(b); return hex.EncodeToString(v[:]) }
	defer func() {
		receipt["passed"] = !t.Failed()
		receipt["checks"] = checks
		receipt["views"] = views
		receipt["expiry_isolation"] = expirySummary
		b, e := json.MarshalIndent(receipt, "", "  ")
		if e != nil {
			t.Error(e)
			return
		}
		for _, forbidden := range []string{
			"uc_dc_", "synth-secret", "synth-jwt", "host-only-synth",
			"desktop-only-synth", "selfhost-synth", "manual-official-synth",
			"exp-env-key", "exp-toml-key",
		} {
			if bytes.Contains(b, []byte(forbidden)) {
				t.Errorf("redaction violation in receipt JSON: %q", forbidden)
			}
		}
		if e = os.WriteFile(filepath.Join(out, "settings-cloud-relay.json"), append(b, '\n'), 0600); e != nil {
			t.Error(e)
		}
		if rpath := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"); rpath != "" {
			if e = os.MkdirAll(filepath.Dir(rpath), 0700); e != nil {
				t.Error(e)
				return
			}
			if e = os.WriteFile(rpath, append(b, '\n'), 0600); e != nil {
				t.Error(e)
			}
		}
	}()

	exe := filepath.Join(private, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if bout, e := build.CombinedOutput(); e != nil {
		t.Fatalf("build unicd: %v\n%s", e, bout)
	}
	binary, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	receipt["unicd_sha256"] = hash(binary)

	const (
		managedCred     = "uc_dc_synth0001-synth-secret-value"
		managedCredA    = "uc_dc_synthA001-synth-secret-value"
		managedCredB    = "uc_dc_synthB002-synth-secret-value"
		hostOnlyKey     = "host-only-synth-key"
		desktopOnlyKey  = "desktop-only-synth-key"
		selfhostKey     = "selfhost-synth-key-only"
		manualOfficialK = "manual-official-synth-key"
		expEnvKey       = "exp-env-key-only"
		expTomlKey      = "exp-toml-key-only"
	)

	classifyKey := func(k string) string {
		switch k {
		case "":
			return "none"
		case managedCred:
			return "managed"
		case managedCredA:
			return "managed_A"
		case managedCredB:
			return "managed_B"
		case hostOnlyKey:
			return "static_host_env"
		case selfhostKey:
			return "static_selfhost"
		case expEnvKey:
			return "static_env"
		case expTomlKey:
			return "static_toml"
		default:
			if strings.HasPrefix(k, "uc_dc_") {
				return "managed_other"
			}
			return "other"
		}
	}

	var (
		traceMu             sync.Mutex
		relayTraces         []relayTraceEntry
		officialManagedHits atomic.Int64
		officialHostEnvHits atomic.Int64
		selfhostManagedLeak atomic.Int64
		selfhostKeyHits     atomic.Int64
		untrustedHTTPReqs   atomic.Int64
	)
	recordRelay := func(target, kind, key string) {
		traceMu.Lock()
		relayTraces = append(relayTraces, relayTraceEntry{
			Target:   target,
			Kind:     kind,
			CredKind: classifyKey(key),
			AtMs:     time.Now().UnixMilli(),
		})
		traceMu.Unlock()
	}
	snapshotTracesSince := func(idx int) []relayTraceEntry {
		traceMu.Lock()
		defer traceMu.Unlock()
		return append([]relayTraceEntry(nil), relayTraces[idx:]...)
	}
	traceLen := func() int {
		traceMu.Lock()
		defer traceMu.Unlock()
		return len(relayTraces)
	}

	relayHandler := func(target string, isSelfhost bool) http.HandlerFunc {
		return func(w http.ResponseWriter, r *http.Request) {
			key := r.Header.Get("X-Unic-Relay-Key")
			kind := "control"
			if strings.HasSuffix(r.URL.Path, "/events") {
				kind = "sse"
			}
			recordRelay(target, kind, key)

			if isSelfhost {
				if strings.HasPrefix(key, "uc_dc_") {
					selfhostManagedLeak.Add(1)
				}
				if key == selfhostKey {
					selfhostKeyHits.Add(1)
				}
			} else {
				switch key {
				case managedCred, managedCredA, managedCredB:
					officialManagedHits.Add(1)
				case hostOnlyKey:
					officialHostEnvHits.Add(1)
				}
			}
			switch {
			case strings.HasSuffix(r.URL.Path, "/transfers"):
				w.Header().Set("Content-Type", "application/json")
				_, _ = w.Write([]byte(`{"transfers":[]}`))
			case strings.HasSuffix(r.URL.Path, "/events"):
				w.Header().Set("Content-Type", "text/event-stream")
				w.WriteHeader(http.StatusOK)
				if fl, ok := w.(http.Flusher); ok {
					fl.Flush()
				}
				<-r.Context().Done()
			default:
				http.NotFound(w, r)
			}
		}
	}

	officialRelayA := httptest.NewTLSServer(relayHandler("A", false))
	defer func() {
		cancel()
		officialRelayA.CloseClientConnections()
		officialRelayA.Close()
	}()
	officialRelayB := httptest.NewTLSServer(relayHandler("B", false))
	defer func() {
		cancel()
		officialRelayB.CloseClientConnections()
		officialRelayB.Close()
	}()
	selfhostRelaySrv := httptest.NewTLSServer(relayHandler("selfhost", true))
	defer func() {
		cancel()
		selfhostRelaySrv.CloseClientConnections()
		selfhostRelaySrv.Close()
	}()
	untrustedHTTP := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		untrustedHTTPReqs.Add(1)
		http.Error(w, "untrusted http", http.StatusForbidden)
	}))
	defer func() {
		cancel()
		untrustedHTTP.CloseClientConnections()
		untrustedHTTP.Close()
	}()

	var (
		credSeq        atomic.Int32
		revokeCalls    atomic.Int32
		gateMu         sync.Mutex
		provisionCh    chan struct{}
		cloudReturn503 atomic.Bool
		cloudNextURL   atomic.Value // string
		cloudNextCred  atomic.Value // string
		cloudNextTTL   atomic.Int64 // ms
	)
	cloudNextURL.Store(officialRelayA.URL)
	cloudNextCred.Store(managedCred)
	cloudNextTTL.Store(int64((24 * time.Hour) / time.Millisecond))

	setProvisionGate := func(ch chan struct{}) {
		gateMu.Lock()
		provisionCh = ch
		gateMu.Unlock()
	}
	cloudSrv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/api/auth/refresh" && r.Method == http.MethodPost:
			if cloudReturn503.Load() {
				http.Error(w, "temporarily unavailable", http.StatusServiceUnavailable)
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token":  "synth-jwt-token",
				"refresh_token": "synth-refresh-token",
				"expires_in":    900,
			})
		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodPost:
			gateMu.Lock()
			ch := provisionCh
			gateMu.Unlock()
			if ch != nil {
				select {
				case <-ch:
				case <-r.Context().Done():
					return
				}
			}
			if cloudReturn503.Load() {
				w.WriteHeader(http.StatusServiceUnavailable)
				_ = json.NewEncoder(w).Encode(map[string]string{"error": "service_unavailable"})
				return
			}
			seq := credSeq.Add(1)
			ttlMs := cloudNextTTL.Load()
			now := time.Now()
			advURL, _ := cloudNextURL.Load().(string)
			credVal, _ := cloudNextCred.Load().(string)
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":                 fmt.Sprintf("dc-e2e-%d", seq),
				"key_prefix":         "uc_dc_synth",
				"credential":         credVal,
				"scopes":             []string{"relay"},
				"relay_url":          advURL,
				"max_transfer_bytes": 25 << 20,
				"expires_at_ms":      now.Add(time.Duration(ttlMs) * time.Millisecond).UnixMilli(),
				"rotate_after_ms":    now.Add(12 * time.Hour).UnixMilli(),
			})
		case strings.HasPrefix(r.URL.Path, "/api/v1/device-credentials/") && r.Method == http.MethodDelete:
			revokeCalls.Add(1)
			w.WriteHeader(http.StatusOK)
			_ = json.NewEncoder(w).Encode(map[string]any{"revoked": true})
		default:
			http.NotFound(w, r)
		}
	}))
	defer func() {
		cancel()
		cloudSrv.CloseClientConnections()
		cloudSrv.Close()
	}()

	// Test-only trusted CA bundle passed solely to isolated test daemon processes
	// and desktop test HTTP client; zero plain-HTTP production exception exists.
	caPath := filepath.Join(private, "test-ca.pem")
	var caBuf bytes.Buffer
	for _, srv := range []*httptest.Server{officialRelayA, officialRelayB, selfhostRelaySrv, cloudSrv} {
		if cert := srv.Certificate(); cert != nil {
			_ = pem.Encode(&caBuf, &pem.Block{Type: "CERTIFICATE", Bytes: cert.Raw})
		}
	}
	if err = os.WriteFile(caPath, caBuf.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}

	baseEnv := []string{}
	for _, v := range os.Environ() {
		if !strings.HasPrefix(v, "UNIC_") && !strings.HasPrefix(v, "SSL_CERT_FILE=") {
			baseEnv = append(baseEnv, v)
		}
	}
	startDaemon := func(name, initialTOML string, extraEnv ...string) (string, *api.Client, func()) {
		home := filepath.Join(private, name)
		if e := os.MkdirAll(home, 0700); e != nil {
			t.Fatal(e)
		}
		if initialTOML == "" {
			initialTOML = "[telemetry]\nenabled = false\n[sync]\nenabled = true\n"
		}
		if e := os.WriteFile(filepath.Join(home, "config.toml"), []byte(initialTOML), 0600); e != nil {
			t.Fatal(e)
		}
		var logs bytes.Buffer
		cmd := exec.CommandContext(ctx, exe)
		env := append(slices.Clone(baseEnv),
			"UNIC_HOME="+home,
			"UNIC_RENDEZVOUS_URL=http://127.0.0.1:1",
			"SSL_CERT_FILE="+caPath,
			// Fail fast if a fallback ever reaches external relay.uniclipboard.app in offline tests.
			"HTTPS_PROXY=http://127.0.0.1:1",
			"NO_PROXY=127.0.0.1,localhost",
		)
		env = append(env, extraEnv...)
		cmd.Env = env
		cmd.Stdout = &logs
		cmd.Stderr = &logs
		if e := cmd.Start(); e != nil {
			t.Fatal(e)
		}
		cli := api.NewClient(filepath.Join(home, "unic.sock"))
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			if _, e := cli.Status(ctx); e == nil {
				break
			}
			time.Sleep(20 * time.Millisecond)
		}
		if _, e := cli.Status(ctx); e != nil {
			_ = cmd.Process.Kill()
			_ = cmd.Wait()
			t.Fatalf("unicd %s failed to start: %v\n%s", name, e, logs.String())
		}
		stop := func() {
			_ = cmd.Process.Kill()
			_ = cmd.Wait()
			if t.Failed() {
				t.Logf("[%s logs]\n%s", name, logs.String())
			}
		}
		return home, cli, stop
	}

	wait := func(label string, f func() bool) {
		t.Helper()
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			if f() {
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
		t.Fatalf("timeout waiting for: %s", label)
	}

	if err = boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}

	newBoundDesktop := func(cli *api.Client, loginCalls *atomic.Int32) (*Desktop, *ui.Tester, func(string, func() bool), func(string), func(string), func(string) string) {
		desktop := NewDesktop(ctx, cli)
		d := desktop.d
		d.cloud.httpClient = cloudSrv.Client()
		d.cloud.loginFn = func(_ context.Context, _ cloudauth.LoginOptions) (cloudauth.Session, error) {
			if loginCalls != nil {
				loginCalls.Add(1)
			}
			return cloudauth.Session{
				State:        cloudauth.SessionStateActive,
				CloudURL:     cloudSrv.URL,
				AccessToken:  "synth-jwt-token",
				RefreshToken: "synth-refresh-token",
				User:         cloudauth.User{Email: "alice@example.com"},
				SignedInAtMs: 1764200000000,
			}, nil
		}
		pump := func(label string, cond func() bool) {
			t.Helper()
			wait(label, func() bool {
				d.live.poll(d)
				return cond()
			})
		}
		pump("desktop initial load", func() bool {
			return d.live.loaded && !d.live.loading && !d.live.busy
		})
		d.page = "settings"
		d.settings.Value = "sync"
		tt := ui.NewTesterWith(d.view, 1200, 920, ui.TesterOptions{})
		clickSwitch := func(label string) {
			t.Helper()
			n, ok := tt.AccessByRole(ui.RoleSwitch, label)
			if !ok {
				t.Fatalf("missing switch %q", label)
			}
			tt.ClickAt(n.Bounds.X+n.Bounds.W/2, n.Bounds.Y+n.Bounds.H/2)
		}
		clickEnabledButton := func(label string) {
			t.Helper()
			var matches []ui.AccessInfo
			for _, a := range tt.AccessAll() {
				if a.Role == ui.RoleButton && a.Name == label && !a.Disabled {
					matches = append(matches, a)
				}
			}
			if len(matches) == 0 {
				t.Fatalf("missing enabled button %q", label)
			}
			n := matches[len(matches)-1]
			tt.ClickAt(n.Bounds.X+n.Bounds.W/2, n.Bounds.Y+n.Bounds.H/2)
		}
		capture := func(name string) string {
			t.Helper()
			tt.Frame()
			texts := strings.Join(tt.Texts(), " | ")
			assertAbsentTexts(t, texts, "uc_dc_", "synth-secret", "synth-jwt", "synth-refresh",
				selfhostKey, hostOnlyKey, desktopOnlyKey, manualOfficialK, expEnvKey, expTomlKey)
			path := filepath.Join(out, name+".png")
			f, e := os.Create(path)
			if e != nil {
				t.Fatal(e)
			}
			encErr := png.Encode(f, tt.Image())
			closeErr := f.Close()
			if encErr != nil {
				t.Fatal(encErr)
			}
			if closeErr != nil {
				t.Fatal(closeErr)
			}
			views = append(views, map[string]any{
				"id":        name,
				"width_px":  tt.Image().Bounds().Dx(),
				"height_px": tt.Image().Bounds().Dy(),
				"text_hits": len(tt.Texts()),
			})
			return texts
		}
		return desktop, tt, pump, clickSwitch, clickEnabledButton, capture
	}

	home, client, stopMain := startDaemon("home-main", "", "UNIC_CLOUD_URL="+cloudSrv.URL)
	defer stopMain()

	// Check 0a: Legacy GET /v1/config response omitting env_access_key_set defaults cleanly to false.
	legacySock := filepath.Join(private, "legacy.sock")
	legacyLn, lerr := net.Listen("unix", legacySock)
	if lerr != nil {
		t.Fatal(lerr)
	}
	legacyMux := http.NewServeMux()
	legacyMux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(api.Status{ID: "legacy-1", Name: "Legacy"})
	})
	legacyHome := filepath.Join(private, "legacy-home")
	_ = os.MkdirAll(legacyHome, 0o700)
	legacyMux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"revision": "r-old",
			"path":     filepath.Join(legacyHome, "config.toml"),
			"config": map[string]any{
				"desktop": map[string]any{
					"language":       "zh-CN",
					"color_mode":     "system",
					"tray_icon":      "glyph",
					"mode":           "normal",
					"update_channel": "stable",
				},
				"sync": map[string]any{
					"enabled": true,
					"relay": map[string]any{
						"enabled": false,
					},
				},
			},
		})
	})
	legacyMux.HandleFunc("GET /v1/history/tags", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`[]`))
	})
	legacyMux.HandleFunc("GET /v1/candidates", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`[]`))
	})
	legacyMux.HandleFunc("GET /v1/actions", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`[]`))
	})
	legacyMux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`[]`))
	})
	legacyMux.HandleFunc("GET /v1/events", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)
		if f, ok := w.(http.Flusher); ok {
			f.Flush()
		}
		<-r.Context().Done()
	})
	legacyMux.HandleFunc("POST /v1/history/search/counts", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"counts": []uint64{}})
	})
	legacyMux.HandleFunc("POST /v1/history/search/candidates", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"candidates": []any{}})
	})
	legacyMux.HandleFunc("GET /v1/history", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(api.HistoryPage{Items: []api.HistoryItem{}, Revision: 1})
	})
	legacyMux.HandleFunc("POST /v1/history/search", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(api.HistorySearchPage{Hits: []api.HistorySearchHit{}, HistoryRevision: 1})
	})
	legacyMux.HandleFunc("GET /v1/operations", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`[]`))
	})
	legacySrv := &http.Server{Handler: legacyMux}
	go func() { _ = legacySrv.Serve(legacyLn) }()
	t.Setenv("UNIC_RELAY_ACCESS_KEY", desktopOnlyKey)
	var legacyLoginCalls atomic.Int32
	legacyDesktop, legacyTT, _, legacyClickSw, _, _ := newBoundDesktop(api.NewClient(legacySock), &legacyLoginCalls)
	if legacyDesktop.d.live.config.Config.Sync.Relay.EnvAccessKeySet {
		t.Fatal("legacy omitted env_access_key_set must default to false")
	}
	legacyTT.Frame()
	assertAbsentTexts(t, strings.Join(legacyTT.Texts(), " | "), "当前使用手动配置的接入密钥，也可登录云账号。")
	legacyClickSw("启用混合中转")
	legacyDesktop.Close()
	_ = legacySrv.Close()
	if legacyLoginCalls.Load() == 0 {
		t.Fatal("expected omitted env_access_key_set on legacy daemon to trigger login rather than reading desktop env")
	}
	checks = append(checks, "legacy GET /v1/config omitting env_access_key_set defaults false on Desktop")

	// Check 0b: Forged TOML trying to write env_access_key_set = true fails with 400 leaving revision unchanged.
	beforeForge, err := client.Config(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if _, serr := client.SaveConfig(ctx, beforeForge.Revision, []byte("[sync.relay]\nenv_access_key_set = true\n")); serr == nil {
		t.Fatal("expected forged sync.relay.env_access_key_set in PUT /v1/config TOML to return error")
	}
	afterForge, err := client.Config(ctx)
	if err != nil || afterForge.Revision != beforeForge.Revision || afterForge.Config.Sync.Relay.EnvAccessKeySet {
		t.Fatalf("forged TOML altered state: before=%+v after=%+v err=%v", beforeForge.Revision, afterForge, err)
	}
	checks = append(checks, "PUT /v1/config rejects forged sync.relay.env_access_key_set in TOML cleanly without altering revision")

	var loginCalls atomic.Int32
	desktop, tt, pump, clickSwitch, _, capture := newBoundDesktop(client, &loginCalls)
	defer func() {
		if desktop != nil {
			desktop.Close()
		}
	}()
	d := desktop.d

	// 1. Official mode signed-out without host fallback, EVEN when Desktop process has UNIC_RELAY_ACCESS_KEY set.
	t.Setenv("UNIC_RELAY_ACCESS_KEY", desktopOnlyKey)
	t1 := capture("01-official-signed-out")
	assertPresentTexts(t, t1, "混合中转", "启用混合中转", "服务地址", "云服务账号", "官方服务需登录账号后使用。", "登录", "保存")
	assertAbsentTexts(t, t1, "当前使用手动配置的接入密钥，也可登录云账号。")
	assertNoAccessKeyRow(t, tt.Texts())
	if d.live.config.Config.Sync.Relay.EnvAccessKeySet {
		t.Fatal("expected EnvAccessKeySet=false when only Desktop env sets UNIC_RELAY_ACCESS_KEY")
	}
	checks = append(checks, "desktop-only UNIC_RELAY_ACCESS_KEY ignored: no fallback note and no false host readiness")

	// 1b. Non-HTTPS (plain http://) Cloud-advertised relay_url must be rejected cleanly with 0 requests sent to it.
	cloudNextURL.Store(untrustedHTTP.URL)
	clickSwitch("启用混合中转")
	waitErr := time.Now().Add(20 * time.Second)
	for time.Now().Before(waitErr) {
		d.live.poll(d)
		if !d.cloud.busy && d.cloud.err != "" {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if d.cloud.busy || d.cloud.err == "" {
		t.Fatalf("plain-HTTP relay_url not rejected in time: busy=%v err=%q signedIn=%v cred=%+v seq=%d loginCalls=%d relay=%+v dir=%q",
			d.cloud.busy, d.cloud.err, d.cloud.signedIn, d.cloud.session.DeviceCredential, credSeq.Load(), loginCalls.Load(), d.live.config.Config.Sync.Relay, d.cloudProfileDir())
	}
	if untrustedHTTPReqs.Load() != 0 || d.cloud.session.DeviceCredential != nil {
		t.Fatalf("expected untrusted HTTP relay_url rejected with 0 requests, hits=%d cred=%+v", untrustedHTTPReqs.Load(), d.cloud.session.DeviceCredential)
	}
	cloudNextURL.Store(officialRelayA.URL)
	cloudNextCred.Store(managedCred)
	if delErr := cloudauth.DeleteSession(home); delErr != nil {
		t.Fatal(delErr)
	}
	d.cloud.lastStatAt = time.Time{}
	d.cloud.signedIn = false
	d.cloud.session = cloudauth.Session{}
	d.cloud.err = ""
	loginCalls.Store(0)
	checks = append(checks, "non-HTTPS Cloud-advertised relay_url rejected cleanly with zero HTTP requests sent")

	// 2. Toggle ON in official mode while signed out + gate Cloud provisioning across <120ms and >=120ms.
	gate := make(chan struct{})
	var gateOnce sync.Once
	releaseGate := func() {
		gateOnce.Do(func() {
			close(gate)
			setProvisionGate(nil)
		})
	}
	defer releaseGate()
	setProvisionGate(gate)
	clickSwitch("启用混合中转")
	if !d.cloud.busy || d.cloud.busySince.IsZero() {
		t.Fatalf("expected d.cloud.busy=true with non-zero busySince immediately on click, got busy=%v since=%v", d.cloud.busy, d.cloud.busySince)
	}
	d.cloud.busySince = time.Now().Add(5 * time.Second)
	tt.Frame()
	if d.cloud.showBusy() {
		t.Fatal("expected d.cloud.showBusy()=false inside <120ms window")
	}
	swFast, ok := tt.AccessByRole(ui.RoleSwitch, "启用混合中转")
	if !ok || swFast.Disabled {
		t.Fatalf("expected relay switch visually enabled inside <120ms Cloud busy window, ok=%v disabled=%v", ok, swFast.Disabled)
	}
	clickSwitch("启用混合中转")
	d.cloud.busySince = time.Now().Add(-foregroundLoadingIndicatorDelay - 20*time.Millisecond)
	tt.Frame()
	if !d.cloud.showBusy() {
		t.Fatal("expected d.cloud.showBusy()=true after >=120ms")
	}
	swSlow, ok := tt.AccessByRole(ui.RoleSwitch, "启用混合中转")
	if !ok || !swSlow.Disabled || swSlow.Bounds != swFast.Bounds {
		t.Fatalf("expected relay switch visually disabled after >=120ms with identical bounds, fast=%+v slow=%+v", swFast.Bounds, swSlow.Bounds)
	}
	releaseGate()

	pump("official sign-in + TLS officialRelayA provision", func() bool {
		return !d.cloud.busy && d.cloud.signedIn && !d.live.busy && d.live.config.Config.Sync.Relay.Enabled
	})
	if loginCalls.Load() != 1 {
		t.Fatalf("expected exactly 1 Cloud login call, got %d", loginCalls.Load())
	}
	t2 := capture("02-official-signed-in")
	assertPresentTexts(t, t2, "混合中转", "已登录 alice@example.com", "已登录", "退出登录")
	assertNoAccessKeyRow(t, tt.Texts())

	cfgState, err := client.Config(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if !cfgState.Config.Sync.Relay.Enabled || cfgState.Config.Sync.Relay.URL != "" {
		t.Fatalf("expected sync.relay.url empty in official mode even with TLS non-default Cloud URL, got %+v", cfgState.Config.Sync.Relay)
	}
	wait("unicd sends first authenticated request to Cloud-advertised TLS officialRelayA", func() bool {
		return officialManagedHits.Load() > 0
	})
	tomlBytes, err := os.ReadFile(filepath.Join(home, "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(tomlBytes, []byte("uc_dc_")) || bytes.Contains(tomlBytes, []byte("synth-secret")) {
		t.Fatal("managed cloud credential leaked into config.toml")
	}
	t.Setenv("UNIC_RELAY_ACCESS_KEY", "")
	checks = append(checks, "<120ms vs >=120ms Cloud busy states verified; TLS non-default official relay_url kept sync.relay.url empty while unicd authenticated first request with managed credential")

	// 3. Reopening Desktop preserves Official mode and signed-in state without self-hosted controls.
	desktop.Close()
	desktop, tt, pump, clickSwitch, clickEnabledButton, capture := newBoundDesktop(client, &loginCalls)
	d = desktop.d
	t3Reopen := capture("02b-official-reopened")
	assertPresentTexts(t, t3Reopen, "已登录 alice@example.com", "已登录", "退出登录")
	assertNoAccessKeyRow(t, tt.Texts())
	if d.live.config.Config.Sync.Relay.URL != "" {
		t.Fatalf("expected empty official URL after reopen, got %q", d.live.config.Config.Sync.Relay.URL)
	}

	d.settings.Value = "cloud"
	t3 := capture("03-cloud-account-signed-in")
	assertPresentTexts(t, t3, "alice@example.com", "退出登录", "混合中转", "已开启", "设置")
	checks = append(checks, "reopened Desktop and Cloud tab keep official state with TLS advertised relay_url")

	// 4. Simulate background session expiration (reauth_required) cleanly detected by desktopui.
	d.settings.Value = "sync"
	sess, ok, err := cloudauth.LoadSession(home)
	if err != nil || !ok {
		t.Fatalf("LoadSession: ok=%v err=%v", ok, err)
	}
	sess.State = cloudauth.SessionStateReauthRequired
	if err = cloudauth.SaveSession(home, sess); err != nil {
		t.Fatal(err)
	}
	future := time.Now().Add(2 * time.Second)
	_ = os.Chtimes(cloudauth.SessionPath(home), future, future)
	d.cloud.lastStatAt = time.Time{}
	t4 := capture("04-official-reauth-required")
	assertPresentTexts(t, t4, "需重新登录", "重新登录", "登录已失效，请重新登录。")
	assertNoAccessKeyRow(t, tt.Texts())
	d.settings.Value = "cloud"
	t4Cloud := capture("04b-cloud-reauth-required")
	assertPresentTexts(t, t4Cloud, "需重新登录", "重新登录", "登录已失效，请重新登录以恢复官方服务。")
	checks = append(checks, "external reauth_required session state updates Sync and Cloud panels without exposing secrets")

	// 5. Re-authenticate via 重新登录, then log out -> official relay flips off when unbacked.
	d.settings.Value = "sync"
	tt.Frame()
	clickEnabledButton("重新登录")
	pump("reauth login + provision", func() bool {
		return !d.cloud.busy && d.cloud.signedIn && !d.live.busy && credSeq.Load() >= 2
	})
	tt.Frame()
	clickEnabledButton("退出登录")
	pump("logout disables official relay", func() bool {
		return !d.cloud.busy && !d.cloud.signedIn && !d.live.busy && !d.live.config.Config.Sync.Relay.Enabled
	})
	if revokeCalls.Load() == 0 {
		t.Fatal("expected logout to attempt remote credential revocation")
	}
	checks = append(checks, "reauth recovers and logout revokes credential and disables unbacked official relay")

	// 6. Configure explicit self-hosted TLS URL while signed out -> access key row appears and managed cred never hits selfhost.
	d.relay.urlDraft = selfhostRelaySrv.URL + "/"
	tt.Frame()
	clickEnabledButton("保存")
	pump("save selfhost URL", func() bool {
		return !d.live.busy && d.live.config.Config.Sync.Relay.URL == selfhostRelaySrv.URL
	})
	t5 := capture("05-selfhost-no-key")
	assertPresentTexts(t, t5, "服务地址", "恢复默认", "接入密钥", "未配置", "填写密钥", "用于自建中转服务鉴权（仅写入不回显）。")
	assertAbsentTexts(t, t5, "云服务账号", "退出登录", "登录账号")

	clickEnabledButton("填写密钥")
	tt.Frame()
	d.relay.accessKeyDraft = selfhostKey
	tt.Frame()
	clickEnabledButton("保存")
	pump("save selfhost access key", func() bool {
		return !d.live.busy && d.live.config.Config.Sync.Relay.AccessKeySet
	})
	t6 := capture("06-selfhost-configured")
	assertPresentTexts(t, t6, "已配置", "更换", "清除", "恢复默认")
	clickSwitch("启用混合中转")
	pump("enable selfhost relay", func() bool {
		return !d.live.busy && d.live.config.Config.Sync.Relay.Enabled
	})
	wait("selfhost relay authenticated with user key and zero managed leaks", func() bool {
		return selfhostKeyHits.Load() > 0
	})
	if selfhostManagedLeak.Load() != 0 {
		t.Fatalf("managed credential leaked to selfhost relay %d times", selfhostManagedLeak.Load())
	}
	checks = append(checks, "self-hosted mode shows write-only access-key controls, hides Cloud row, and never sends managed credentials")

	// 7. Reset SelfHost -> Official via 恢复默认 clears key controls and disables unbacked official relay.
	clickEnabledButton("恢复默认")
	pump("reset back to official", func() bool {
		return !d.live.busy && d.live.config.Config.Sync.Relay.URL == "" && !d.live.config.Config.Sync.Relay.Enabled && !d.live.config.Config.Sync.Relay.AccessKeySet
	})
	capture("07-reset-to-official")

	// 8. Manual TOML key fallback in official mode preserves hidden key row, shows note, and flips directly.
	rawManual, err := config.Set(d.live.config.Config, "sync.relay.access_key", `"`+manualOfficialK+`"`)
	if err != nil {
		t.Fatal(err)
	}
	manualState, err := client.SaveConfig(ctx, d.live.config.Revision, rawManual)
	if err != nil {
		t.Fatal(err)
	}
	d.live.config = manualState
	d.projectLiveConfig()
	t8 := capture("08-official-manual-fallback")
	assertPresentTexts(t, t8, "当前使用手动配置的接入密钥，也可登录云账号。", "登录")
	assertNoAccessKeyRow(t, tt.Texts())
	clickSwitch("启用混合中转")
	pump("enable official relay with manual fallback", func() bool {
		return !d.live.busy && d.live.config.Config.Sync.Relay.Enabled && d.live.config.Config.Sync.Relay.AccessKeySet && !d.cloud.busy
	})
	desktop.Close()
	desktop = nil
	checks = append(checks, "official manual TOML fallback omits key editor row and permits direct toggle")

	// 9. Isolated host-only UNIC_RELAY_ACCESS_KEY check: host sets env, Desktop env stays empty.
	officialHostEnvHits.Store(0)
	var hostLoginCalls atomic.Int32
	homeHost, clientHost, stopHost := startDaemon(
		"home-host-env",
		"",
		"UNIC_RELAY_ACCESS_KEY="+hostOnlyKey,
		"UNIC_RELAY_URL="+officialRelayA.URL,
		"UNIC_CLOUD_URL="+cloudSrv.URL,
	)
	defer stopHost()

	desktopHost, ttHost, pumpHost, clickHostSwitch, clickHostBtn, captureHost := newBoundDesktop(clientHost, &hostLoginCalls)
	defer func() {
		if desktopHost != nil {
			desktopHost.Close()
		}
	}()
	if !desktopHost.d.live.config.Config.Sync.Relay.EnvAccessKeySet || desktopHost.d.live.config.Config.Sync.Relay.AccessKeySet {
		t.Fatalf("expected EnvAccessKeySet=true and AccessKeySet=false on host-env-only daemon, got %+v", desktopHost.d.live.config.Config.Sync.Relay)
	}
	t9 := captureHost("09-host-only-env-fallback")
	assertPresentTexts(t, t9, "当前使用手动配置的接入密钥，也可登录云账号。", "登录")
	assertNoAccessKeyRow(t, ttHost.Texts())
	clickHostSwitch("启用混合中转")
	pumpHost("enable relay via host-only env fallback", func() bool {
		return !desktopHost.d.live.busy && desktopHost.d.live.config.Config.Sync.Relay.Enabled && !desktopHost.d.cloud.busy
	})
	if hostLoginCalls.Load() != 0 {
		t.Fatalf("expected zero login attempts when host env fallback is present, got %d", hostLoginCalls.Load())
	}
	wait("host-only env relay authenticated request", func() bool {
		return officialHostEnvHits.Load() > 0
	})
	if err = cloudauth.SaveSession(homeHost, cloudauth.Session{
		State:        cloudauth.SessionStateActive,
		CloudURL:     cloudSrv.URL,
		AccessToken:  "synth-jwt-token",
		RefreshToken: "synth-refresh-token",
		User:         cloudauth.User{Email: "alice@example.com"},
		SignedInAtMs: 1764200000000,
	}); err != nil {
		t.Fatal(err)
	}
	desktopHost.d.cloud.lastStatAt = time.Time{}
	ttHost.Frame()
	clickHostBtn("退出登录")
	pumpHost("logout preserves enabled relay with host env fallback", func() bool {
		return !desktopHost.d.cloud.busy && !desktopHost.d.cloud.signedIn && !desktopHost.d.live.busy
	})
	if !desktopHost.d.live.config.Config.Sync.Relay.Enabled || !desktopHost.d.live.config.Config.Sync.Relay.EnvAccessKeySet {
		t.Fatalf("expected relay to stay enabled via host env fallback after logout, got %+v", desktopHost.d.live.config.Config.Sync.Relay)
	}
	desktopHost.Close()
	desktopHost, ttHost, _, _, _, _ = newBoundDesktop(clientHost, &hostLoginCalls)
	ttHost.Frame()
	assertPresentTexts(t, strings.Join(ttHost.Texts(), " | "), "当前使用手动配置的接入密钥，也可登录云账号。", "登录")
	assertNoAccessKeyRow(t, ttHost.Texts())
	if !desktopHost.d.live.config.Config.Sync.Relay.Enabled || !desktopHost.d.live.config.Config.Sync.Relay.EnvAccessKeySet {
		t.Fatalf("expected host env fallback state preserved on reopen, got %+v", desktopHost.d.live.config.Config.Sync.Relay)
	}
	desktopHost.Close()
	desktopHost = nil
	checks = append(checks, "host-only UNIC_RELAY_ACCESS_KEY drives fallback note, direct enable, authenticated request, logout preservation, and reopen without Desktop env")

	// 10. Advertised TLS origin A expiry during Cloud 503 outage -> recovery to TLS origin B
	// across ALL THREE fallback configurations ("none", "env", "toml").
	for _, group := range []struct {
		name      string
		tomlExtra string
		extraEnv  []string
	}{
		{name: "none"},
		{name: "env", extraEnv: []string{"UNIC_RELAY_ACCESS_KEY=" + expEnvKey}},
		{name: "toml", tomlExtra: fmt.Sprintf("access_key = %q\n", expTomlKey)},
	} {
		t.Run("expiry-"+group.name, func(t *testing.T) {
			cloudReturn503.Store(false)
			now := time.Now()
			expAt := now.Add(900 * time.Millisecond)
			tomlCfg := fmt.Sprintf("[telemetry]\nenabled = false\n[sync]\nenabled = true\n[sync.relay]\nenabled = true\n%s", group.tomlExtra)

			ghome := filepath.Join(private, "home-exp-"+group.name)
			if e := os.MkdirAll(ghome, 0700); e != nil {
				t.Fatal(e)
			}
			// Seed active session bound to TLS origin A BEFORE starting daemon; turn Cloud 503 ON immediately.
			if e := cloudauth.SaveSession(ghome, cloudauth.Session{
				State:             cloudauth.SessionStateActive,
				CloudURL:          cloudSrv.URL,
				AccessToken:       "synth-jwt-token",
				AccessExpiresAtMs: now.Add(15 * time.Minute).UnixMilli(),
				RefreshToken:      "synth-refresh-token",
				User:              cloudauth.User{Email: "alice@example.com"},
				SignedInAtMs:      now.UnixMilli(),
				DeviceCredential: &cloudauth.DeviceCredential{
					ID:               "dc-exp-" + group.name,
					KeyPrefix:        "uc_dc_synthA",
					Credential:       managedCredA,
					Scopes:           []string{"relay"},
					RelayURL:         officialRelayA.URL,
					MaxTransferBytes: 25 << 20,
					ExpiresAtMs:      expAt.UnixMilli(),
					RotateAfterMs:    now.Add(200 * time.Millisecond).UnixMilli(),
				},
			}); e != nil {
				t.Fatal(e)
			}
			cloudReturn503.Store(true)
			startIdx := traceLen()

			_, gcli, stopG := startDaemon("home-exp-"+group.name, tomlCfg, append([]string{"UNIC_CLOUD_URL=" + cloudSrv.URL}, group.extraEnv...)...)
			defer stopG()

			wait(group.name+" initial request to A with managed_A", func() bool {
				for _, tr := range snapshotTracesSince(startIdx) {
					if tr.Target == "A" && tr.CredKind == "managed_A" {
						return true
					}
				}
				return false
			})

			// Wait until past cred_A expiry + pushSettings reaction, then observe 1.5s outage window at A.
			if rem := time.Until(expAt.Add(150 * time.Millisecond)); rem > 0 {
				time.Sleep(rem)
			}
			obsStart := traceLen()
			time.Sleep(1500 * time.Millisecond)
			outageTraces := snapshotTracesSince(obsStart)

			aCounts := map[string]int{"control": 0, "sse": 0, "static": 0}
			for _, tr := range outageTraces {
				if tr.Target == "A" {
					aCounts[tr.Kind]++
					if strings.HasPrefix(tr.CredKind, "static") || tr.CredKind == "none" {
						aCounts["static"]++
					}
				}
			}
			if aCounts["control"] != 0 || aCounts["sse"] != 0 || aCounts["static"] != 0 {
				t.Fatalf("%s: expected ZERO post-expiry control/SSE/static requests at expired A, got %+v (traces=%+v)", group.name, aCounts, outageTraces)
			}

			// Verify Official mode config and active signed-in session survived outage.
			stG, e := gcli.Config(ctx)
			if e != nil {
				t.Fatal(e)
			}
			if stG.Config.Sync.Relay.URL != "" || !stG.Config.Sync.Relay.Enabled {
				t.Fatalf("%s: expected official empty URL and enabled=true during outage, got %+v", group.name, stG.Config.Sync.Relay)
			}
			loadedSess, okSess, e := cloudauth.LoadSession(ghome)
			if e != nil || !okSess || loadedSess.EffectiveState() != cloudauth.SessionStateActive {
				t.Fatalf("%s: expected active Cloud session preserved, ok=%v err=%v state=%q", group.name, okSess, e, loadedSess.EffectiveState())
			}

			// Recover Cloud -> issue cred_B bound to HTTPS origin B.
			recIdx := traceLen()
			cloudNextURL.Store(officialRelayB.URL)
			cloudNextCred.Store(managedCredB)
			cloudNextTTL.Store(int64((time.Hour) / time.Millisecond))
			cloudReturn503.Store(false)

			var bFirst string
			bCounts := map[string]int{"control": 0, "sse": 0}
			wait(group.name+" recovered first request at TLS B", func() bool {
				for _, tr := range snapshotTracesSince(recIdx) {
					if tr.Target == "B" {
						bFirst = tr.CredKind
						return true
					}
				}
				return false
			})
			for _, tr := range snapshotTracesSince(recIdx) {
				if tr.Target == "B" {
					bCounts[tr.Kind]++
					if strings.HasPrefix(tr.CredKind, "static") {
						t.Fatalf("%s: static fallback leaked to recovered origin B: %+v", group.name, tr)
					}
				}
			}
			if bFirst != "managed_B" {
				t.Fatalf("%s: expected first recovered request at B to carry managed_B, got %q", group.name, bFirst)
			}
			expirySummary[group.name] = map[string]any{
				"post_expiry_A_control": aCounts["control"],
				"post_expiry_A_sse":     aCounts["sse"],
				"post_expiry_A_static":  aCounts["static"],
				"recovered_B_first":     bFirst,
				"recovered_B_control":   bCounts["control"],
				"recovered_B_sse":       bCounts["sse"],
				"official_url_empty":    stG.Config.Sync.Relay.URL == "",
			}
		})
	}
	checks = append(checks, "expired HTTPS origin A + 503 outage -> recovery to HTTPS origin B verified with 0 control/SSE/static hits at A across none/env/toml groups")
	_ = errors.Is
	_ = io.Discard
}
