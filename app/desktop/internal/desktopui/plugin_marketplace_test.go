package desktopui

import (
	"bytes"
	"context"
	"encoding/json"
	"image"
	"image/png"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestPluginMarketplaceFlowAndZeroConfigStatus(t *testing.T) {
	dir := t.TempDir()
	sock := filepath.Join(dir, "unic.sock")
	ln, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	var mu sync.Mutex
	installed := []api.PluginInfo{}
	const manualRepo = "https://github.com/mkdir700/unic-plugin-wxime"
	manualStarted := make(chan struct{}, 2)
	finishManual := make(chan struct{})
	var installReq api.PluginInstallRequest
	var uninstalledID string
	cfgState := config.State{Revision: "rev-1", Config: config.Default()}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.Status{ID: "svc-1", Name: "Test Mac", Peers: []api.PeerStatus{}, Pending: []api.PendingPeer{}})
	})
	mux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		_ = json.NewEncoder(w).Encode(cfgState)
	})
	mux.HandleFunc("PUT /v1/config", func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Revision string `json:"revision"`
			TOML     string `json:"toml"`
		}
		_ = json.NewDecoder(r.Body).Decode(&body)
		parsed, err := config.Parse([]byte(body.TOML))
		if err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		mu.Lock()
		cfgState.Revision = "rev-2"
		cfgState.Config = parsed
		cfgState.RestartRequired = []string{"plugins"}
		for i := range installed {
			installed[i].Configured = false
			for _, id := range parsed.Plugins.Order {
				if id == installed[i].ID {
					installed[i].Configured = true
				}
			}
		}
		out := cfgState
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(out)
	})
	mux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		_ = json.NewEncoder(w).Encode(installed)
	})
	mux.HandleFunc("GET /v1/plugins/marketplace", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.PluginMarketplace{
			Version: 1,
			Source:  "remote",
			Plugins: []api.MarketplaceEntry{
				{
					ID:            "unic.wenshushu",
					Name:          "文叔叔临时分享",
					Author:        "UniClipboard 官方",
					Official:      true,
					Category:      "share",
					Version:       "0.1.0",
					Summary:       "一键生成 24 小时临时下载链接。",
					Description:   "无需配置，选中任意条目即可分享。",
					Highlights:    []string{"免配置开箱即用"},
					HowTo:         []string{"选中条目", "点击通过文叔叔临时分享"},
					Privacy:       "仅主动触发时上传。",
					ZeroConfig:    true,
					Repository:    "https://github.com/UniClipboard/unic-plugin-wenshushu",
					Ref:           "fc968e8efd8e4ac204194c21c2c08e34e9c26e7e",
					BuildFallback: true,
					Capabilities:  []string{"action"},
				},
			},
		})
	})
	mux.HandleFunc("POST /v1/plugins/install", func(w http.ResponseWriter, r *http.Request) {
		var req api.PluginInstallRequest
		_ = json.NewDecoder(r.Body).Decode(&req)
		if req.Repository == manualRepo {
			manualStarted <- struct{}{}
			select {
			case <-finishManual:
				http.Error(w, "fixture rejected installation", http.StatusBadGateway)
			case <-r.Context().Done():
			}
			return
		}
		mu.Lock()
		installReq = req
		info := api.PluginInfo{
			ID:               "unic.wenshushu",
			ImplementationID: "unic.wenshushu",
			Version:          "0.1.0",
			Kind:             "go",
			Source:           "source",
			Repository:       "github.com/UniClipboard/unic-plugin-wenshushu",
			Commit:           "fc968e8efd8e4ac204194c21c2c08e34e9c26e7e",
			Capabilities:     []string{"action"},
			Actions: []api.PluginActionInfo{{
				ID:      "share",
				Key:     "unic.wenshushu/share",
				Title:   "通过文叔叔临时分享",
				Accepts: []string{"text", "image", "files"},
			}},
			Configured:     false,
			RunningVersion: "0.1.0",
			HasSettings:    false,
		}
		installed = []api.PluginInfo{info}
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(info)
	})
	mux.HandleFunc("DELETE /v1/plugins/{id}", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		uninstalledID = r.PathValue("id")
		installed = nil
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
	})
	mux.HandleFunc("GET /v1/actions", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.Action{}) })
	mux.HandleFunc("GET /v1/sync-backends", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.SyncBackendInfo{}) })
	mux.HandleFunc("GET /v1/history", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.HistoryPage{Items: []api.HistoryItem{}})
	})
	mux.HandleFunc("POST /v1/history/search", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.HistorySearchPage{Hits: []api.HistorySearchHit{}})
	})
	mux.HandleFunc("GET /v1/history/tags", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]string{}) })
	mux.HandleFunc("GET /v1/candidates", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.Candidate{}) })

	srv := &http.Server{Handler: mux}
	go func() { _ = srv.Serve(ln) }()
	defer func() { _ = srv.Close() }()

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	desktop := NewDesktop(ctx, api.NewClient(sock))
	defer desktop.Close()
	d := desktop.d
	d.page = "settings"
	d.settings.Value = "plugins"

	tt := ui.NewTesterWith(desktop.View, 1120, 960, ui.TesterOptions{})
	wait := func(label string, cond func() bool) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			d.live.poll(d)
			tt.Advance(20 * time.Millisecond)
			if cond() {
				return
			}
			time.Sleep(15 * time.Millisecond)
		}
		t.Fatalf("timeout waiting for %s", label)
	}

	wait("initial live & marketplace load", func() bool {
		return d.live.loaded && d.live.marketplaceLoaded && tt.HasText("插件市场") && tt.HasText("文叔叔临时分享")
	})

	// Open the Obsidian-style online marketplace view.
	if err := tt.Click("浏览插件市场"); err != nil {
		t.Fatalf("click 浏览插件市场: %v", err)
	}
	wait("marketplace view", func() bool {
		return d.pluginSettings.view == "marketplace" && tt.HasText("如何使用") && tt.HasText("隐私与安全")
	})

	// Hold a manual installation so its pending state can be observed.
	if err := tt.Click("开发者：从链接手动安装"); err != nil {
		t.Fatal(err)
	}
	wait("manual install dialog", func() bool { return tt.HasText("仓库链接") })
	d.pluginSettings.manualRepo = manualRepo
	tt.Advance(350 * time.Millisecond)
	if err := tt.Click("安装"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-manualStarted:
	case <-time.After(5 * time.Second):
		t.Fatal("manual install did not reach the service")
	}
	tt.Advance(350 * time.Millisecond)
	bounds, ok := tt.Find("正在安装…")
	if !ok {
		t.Fatal("manual submit did not show its loading label")
	}
	crop := image.Rect(int(bounds.X)-24, int(bounds.Y)-2, int(bounds.X+bounds.W), int(bounds.Y+bounds.H)+2)
	capture := func(name string) []byte {
		t.Helper()
		frame := tt.Image()
		if !crop.In(frame.Bounds()) {
			t.Fatal("loading button is outside the rendered frame")
		}
		var bytesOut bytes.Buffer
		if err := png.Encode(&bytesOut, frame.SubImage(crop)); err != nil {
			t.Fatal(err)
		}
		if dir := os.Getenv("UNIC_MANUAL_INSTALL_ARTIFACTS"); dir != "" {
			var full bytes.Buffer
			if err := png.Encode(&full, frame); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(dir, name), full.Bytes(), 0600); err != nil {
				t.Fatal(err)
			}
		}
		return bytesOut.Bytes()
	}
	first := capture("manual-install-loading-1.png")
	tt.Advance(150 * time.Millisecond)
	if bytes.Equal(first, capture("manual-install-loading-2.png")) {
		t.Fatal("manual install button did not animate while the request was pending")
	}
	tt.ClickAt(bounds.X+bounds.W/2, bounds.Y+bounds.H/2)
	select {
	case <-manualStarted:
		t.Fatal("loading button submitted a second installation")
	default:
	}
	close(finishManual)
	wait("manual install failure restores submit", func() bool {
		return d.pluginSettings.manualOpen && !d.live.busy && tt.HasText("安装失败：") && !tt.HasText("正在安装…")
	})
	if err := tt.Click("取消"); err != nil {
		t.Fatal(err)
	}
	wait("manual install dialog closed", func() bool { return !tt.HasText("仓库链接") })

	// One-click install Wenshushu from the marketplace.
	if err := tt.Click("安装"); err != nil {
		t.Fatalf("click 安装: %v", err)
	}
	wait("install completed", func() bool {
		return !d.live.busy && len(d.live.externalPlugins) == 1 && tt.HasText("启用")
	})
	mu.Lock()
	gotReq := installReq
	mu.Unlock()
	if gotReq.Repository != "https://github.com/UniClipboard/unic-plugin-wenshushu" || !gotReq.Build {
		t.Fatalf("unexpected install request: %+v", gotReq)
	}
	if tt.HasText("选项") {
		t.Fatal("zero-config Wenshushu plugin must not show an Options button")
	}

	// Inspect installed external plugin detail: verify running status is fixed (运行中, not 未运行) and action is visible.
	d.pluginSettings.view = ""
	d.pluginSettings.id = "unic.wenshushu"
	tt.Advance(20 * time.Millisecond)
	if !tt.HasText("运行中") {
		t.Fatalf("expected non-sync running external plugin to show 运行中, texts=%v", tt.Texts())
	}
	if !tt.HasText("通过文叔叔临时分享") {
		t.Fatalf("expected action title in external plugin detail")
	}
	if tt.HasText("插件设置") {
		t.Fatal("zero-config external plugin must not render empty 插件设置 group")
	}

	// Uninstall via confirmation dialog.
	if err := tt.Click("卸载"); err != nil {
		t.Fatalf("click 卸载: %v", err)
	}
	wait("uninstall confirmation dialog", func() bool {
		return d.pluginSettings.uninstallOpen && tt.HasText("卸载「文叔叔临时分享」？")
	})
	// Confirm uninstall in AlertDialog.
	d.uninstallExternalPlugin("unic.wenshushu", "文叔叔临时分享")
	wait("uninstall completed", func() bool {
		return !d.live.busy && len(d.live.externalPlugins) == 0
	})
	mu.Lock()
	gotUninstalled := uninstalledID
	mu.Unlock()
	if gotUninstalled != "unic.wenshushu" {
		t.Fatalf("expected DELETE /v1/plugins/unic.wenshushu, got %q", gotUninstalled)
	}
}
