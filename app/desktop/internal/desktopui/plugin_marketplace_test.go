package desktopui

import (
	"context"
	"encoding/json"
	"net"
	"net/http"
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
