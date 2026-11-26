package desktopui

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"math"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/engine"
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
	marketIcon := "link"
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
		mu.Lock()
		icon := marketIcon
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(api.PluginMarketplace{
			Version: 1,
			Source:  "remote",
			Plugins: []api.MarketplaceEntry{
				{
					ID:            "unic.wenshushu",
					Name:          "文叔叔临时分享",
					Icon:          icon,
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

	card, ok := tt.AccessByRole(ui.RoleButton, "文叔叔临时分享")
	if !ok {
		t.Fatal("marketplace card missing")
	}
	detail, ok := tt.AccessByRole(ui.RoleRegion, "文叔叔临时分享")
	if !ok {
		t.Fatal("marketplace detail missing")
	}
	iconCrops := []image.Rectangle{
		image.Rect(int(card.Bounds.X)+15, int(card.Bounds.Y)+13, int(card.Bounds.X)+55, int(card.Bounds.Y)+53),
		image.Rect(int(detail.Bounds.X)+20, int(detail.Bounds.Y)+18, int(detail.Bounds.X)+66, int(detail.Bounds.Y)+64),
	}
	captureIcons := func(name string) [2][]byte {
		t.Helper()
		tt.Advance(350 * time.Millisecond)
		frame := tt.Image()
		var crops [2][]byte
		for i, crop := range iconCrops {
			var out bytes.Buffer
			if err := png.Encode(&out, frame.SubImage(crop)); err != nil {
				t.Fatal(err)
			}
			crops[i] = out.Bytes()
			if dir := os.Getenv("UNIC_MARKETPLACE_ICON_ARTIFACTS"); dir != "" {
				if err := os.WriteFile(filepath.Join(dir, name+"-"+[]string{"card", "detail"}[i]+".png"), crops[i], 0600); err != nil {
					t.Fatal(err)
				}
			}
		}
		return crops
	}
	refreshIcon := func(next string) {
		mu.Lock()
		marketIcon = next
		mu.Unlock()
		d.ensureMarketplaceLoaded(true)
		wait("refreshed marketplace icon", func() bool {
			return !d.live.marketplaceLoading && d.live.marketplace.Plugins[0].Icon == next
		})
	}
	declared := captureIcons("declared")
	refreshIcon("future-icon")
	unknown := captureIcons("unknown")
	refreshIcon("")
	missing := captureIcons("missing")
	for i, want := range missing {
		if bytes.Equal(declared[i], want) || !bytes.Equal(unknown[i], want) {
			t.Fatalf("icon crop %d must show the declared glyph and use the same fallback for unknown and missing icons", i)
		}
	}

	// Hold a manual installation so its async non-blocking state can be observed.
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
	// Manual dialog closes immediately so the page is not blocked; progress appears in the toast.
	wait("manual install dialog closed and toast visible", func() bool {
		return !d.pluginSettings.manualOpen && !tt.HasText("仓库链接") && !d.live.busy && tt.HasText("正在安装「unic-plugin-wxime」…") && tt.HasText("取消安装")
	})
	tt.Advance(350 * time.Millisecond)
	// Reopening manual install dialog while the same repo is installing shows busy state without allowing duplicate submit.
	if err := tt.Click("开发者：从链接手动安装"); err != nil {
		t.Fatal(err)
	}
	wait("manual install dialog reopened in loading state", func() bool {
		return d.pluginSettings.manualOpen && tt.HasText("正在安装…")
	})
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
		return d.pluginSettings.manualOpen && !d.live.busy && d.pluginInstallTask(manualRepo) == nil && tt.HasText("安装失败：unic-plugin-wxime") && !tt.HasText("正在安装…")
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
	if tt.HasText("前往配置") {
		t.Fatal("zero-config Wenshushu completion toast must not show a Configure button")
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

func TestPluginMarketplaceToolbarAndCardListLayout(t *testing.T) {
	dir := t.TempDir()
	sock := filepath.Join(dir, "unic.sock")
	ln, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	var mu sync.Mutex
	refreshCount := 0
	installed := []api.PluginInfo{
		{
			ID:           "unic.wenshushu",
			Version:      "0.1.1",
			Kind:         "go",
			Capabilities: []string{"action"},
			Configured:   true,
		},
		{
			ID:           "unic.syncclipboard",
			Version:      "0.1.0",
			Kind:         "go",
			Capabilities: []string{"sync_backend"},
			Configured:   false,
			HasSettings:  true,
		},
		{
			ID:           "mkdir700.wxime",
			Version:      "0.1.0",
			Kind:         "go",
			Capabilities: []string{"sync_backend"},
			Configured:   true,
			HasSettings:  true,
		},
	}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.Status{ID: "svc-1", Name: "Test Mac", Peers: []api.PeerStatus{}, Pending: []api.PendingPeer{}})
	})
	mux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(config.State{Revision: "rev-1", Config: config.Default()})
	})
	mux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(installed)
	})
	mux.HandleFunc("GET /v1/plugins/marketplace", func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Query().Get("refresh") == "1" {
			mu.Lock()
			refreshCount++
			mu.Unlock()
		}
		_ = json.NewEncoder(w).Encode(api.PluginMarketplace{
			Version: 1,
			Source:  "remote",
			Plugins: []api.MarketplaceEntry{
				{
					ID:           "unic.wenshushu",
					Name:         "文叔叔临时分享",
					Author:       "UniClipboard 官方",
					Official:     true,
					Category:     "share",
					Version:      "0.1.1",
					Summary:      "将选中的文本、图片或文件一键生成约 24 小时有效的临时下载链接。",
					Description:  "无需配置，选中任意条目即可分享。",
					Highlights:   []string{"免配置开箱即用"},
					HowTo:        []string{"选中条目", "点击通过文叔叔临时分享"},
					Privacy:      "仅主动触发时上传。",
					ZeroConfig:   true,
					Capabilities: []string{"action"},
				},
				{
					ID:           "unic.syncclipboard",
					Name:         "SyncClipboard 同步",
					Author:       "UniClipboard 官方",
					Official:     true,
					Category:     "sync",
					Version:      "0.1.0",
					Summary:      "通过 SyncClipboard 客户端双向同步文字和图片。需要支持外部渠道授权的 UniClipboard v2 宿主。",
					Description:  "为兼容客户端提供文字和图片同步，单条内容上限为 2 MiB。",
					Highlights:   []string{"独立同步插件", "每个客户端使用独立渠道凭据"},
					HowTo:        []string{"在兼容宿主上安装插件", "按插件仓库说明配置密钥、启用插件并重启服务"},
					Privacy:      "渠道凭据决定客户端可访问的内容。",
					Capabilities: []string{"sync_backend"},
				},
				{
					ID:           "mkdir700.wxime",
					Name:         "微信输入法剪贴板",
					Author:       "mkdir700",
					Category:     "sync",
					Version:      "0.1.0",
					Summary:      "连接微信输入法跨设备剪贴板分组，双向同步文字和图片。需要支持外部渠道配对的 UniClipboard v2 宿主。",
					Description:  "连接微信输入法剪贴板分组。",
					Highlights:   []string{"跨设备同步"},
					HowTo:        []string{"安装并启用插件"},
					Privacy:      "仅同步配对分组内容。",
					Capabilities: []string{"sync_backend"},
				},
			},
		})
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

	shotDir := os.Getenv("UNIC_MARKETPLACE_LAYOUT_SHOTS")
	saveShot := func(tt *ui.Tester, name string) {
		if shotDir == "" {
			return
		}
		if err := os.MkdirAll(shotDir, 0755); err != nil {
			t.Fatal(err)
		}
		var out bytes.Buffer
		if err := png.Encode(&out, tt.Image()); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(shotDir, name), out.Bytes(), 0644); err != nil {
			t.Fatal(err)
		}
	}

	for _, dark := range []bool{true, false} {
		theme := "light"
		if dark {
			theme = "dark"
		}
		t.Run(theme, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			desktop := NewDesktop(ctx, api.NewClient(sock))
			defer desktop.Close()
			d := desktop.d
			d.theme = theme
			d.page = "settings"
			d.settings.Value = "plugins"
			d.pluginSettings.view = "marketplace"
			d.pluginSettings.marketSelected = "unic.syncclipboard"

			tt := ui.NewTesterWith(desktop.View, 1120, 860, ui.TesterOptions{Dark: dark})
			tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 78})
			deadline := time.Now().Add(5 * time.Second)
			for time.Now().Before(deadline) {
				d.live.poll(d)
				tt.Advance(20 * time.Millisecond)
				if d.live.loaded && d.live.marketplaceLoaded && tt.HasText("微信输入法剪贴板") {
					break
				}
				time.Sleep(15 * time.Millisecond)
			}
			if !d.live.marketplaceLoaded {
				t.Fatal("timeout loading marketplace")
			}

			saveShot(tt, "marketplace-"+theme+".png")

			// Verify Area 1 (back button) and Area 2 (refresh button) match history toolbar's 28px quiet neutral style.
			back, ok := tt.AccessByRole(ui.RoleButton, "返回插件列表")
			if !ok {
				t.Fatal("back button missing")
			}
			if back.Bounds.W != 28 || back.Bounds.H != 28 {
				t.Fatalf("back button bounds=%+v, want 28x28", back.Bounds)
			}
			refresh, ok := tt.AccessByRole(ui.RoleButton, "刷新目录")
			if !ok {
				t.Fatal("refresh button missing")
			}
			if refresh.Bounds.H != 28 {
				t.Fatalf("refresh button height=%v, want 28", refresh.Bounds.H)
			}
			bar, ok := tt.AccessByRole(ui.RoleToolbar, "插件市场")
			if !ok {
				t.Fatal("marketplace toolbar missing")
			}
			// Sample background corner of back & refresh buttons at rest: must match toolbar background (no blue ghost fill).
			im := tt.Image()
			tbBg := im.RGBAAt(int(bar.Bounds.X+bar.Bounds.W/2), int(bar.Bounds.Y+4))
			for _, tc := range []struct {
				name string
				r    ui.Rect
			}{
				{"back", back.Bounds},
				{"refresh", refresh.Bounds},
			} {
				px := im.RGBAAt(int(tc.r.X+2), int(tc.r.Y+2))
				diff := absBoardUIChannel(int(px.R)-int(tbBg.R)) + absBoardUIChannel(int(px.G)-int(tbBg.G)) + absBoardUIChannel(int(px.B)-int(tbBg.B))
				if diff > 12 {
					t.Fatalf("%s button corner pixel=%v differs from toolbar bg=%v (diff=%d); expected transparent rest surface", tc.name, px, tbBg, diff)
				}
			}

			// Verify Area 3: single-column full-width rectangular cards, stacked one per row.
			names := []string{"文叔叔临时分享", "SyncClipboard 同步", "微信输入法剪贴板"}
			cards := make([]ui.Rect, len(names))
			for i, name := range names {
				info, ok := tt.AccessByRole(ui.RoleButton, name)
				if !ok {
					t.Fatalf("card %q missing", name)
				}
				cards[i] = info.Bounds
				if cards[i].W <= cards[i].H {
					t.Fatalf("card %q bounds=%+v is not a horizontal rectangular card", name, cards[i])
				}
				if i > 0 {
					if cards[i].X != cards[0].X || cards[i].W != cards[0].W {
						t.Fatalf("card %q X/W=%v/%v does not match single-column card 0 X/W=%v/%v", name, cards[i].X, cards[i].W, cards[0].X, cards[0].W)
					}
					if cards[i].Y < cards[i-1].Y+cards[i-1].H {
						t.Fatalf("card %q Y=%v overlaps previous row bottom=%v", name, cards[i].Y, cards[i-1].Y+cards[i-1].H)
					}
				}
			}

			// Click refresh in dark run and verify it triggers refresh=1.
			if dark {
				if err := tt.Click("刷新目录"); err != nil {
					t.Fatalf("click refresh: %v", err)
				}
				deadline := time.Now().Add(3 * time.Second)
				for time.Now().Before(deadline) {
					d.live.poll(d)
					tt.Advance(20 * time.Millisecond)
					mu.Lock()
					n := refreshCount
					mu.Unlock()
					if n > 0 && !d.live.marketplaceLoading {
						break
					}
					time.Sleep(15 * time.Millisecond)
				}
				mu.Lock()
				n := refreshCount
				mu.Unlock()
				if n == 0 {
					t.Fatal("clicking 刷新目录 did not issue refresh=1 request")
				}
			}

			// Navigate back to the Plugins list page via the real back button and verify
			// titlebar + installed/builtin row subtle icon buttons are 28px quiet neutral controls.
			if err := tt.Click("返回插件列表"); err != nil {
				t.Fatalf("click back to plugins: %v", err)
			}
			tt.Advance(20 * time.Millisecond)
			saveShot(tt, "plugins-list-"+theme+".png")

			listIm := tt.Image()
			pluginsBar, ok := tt.AccessByRole(ui.RoleToolbar, "插件")
			if !ok {
				t.Fatal("plugins toolbar missing")
			}
			pluginsBarBg := listIm.RGBAAt(int(pluginsBar.Bounds.X+pluginsBar.Bounds.W/2), int(pluginsBar.Bounds.Y+4))
			for _, tc := range []struct {
				name   string
				square bool
			}{
				{"检查更新", false},
				{"更多", true},
			} {
				info, ok := tt.Access(tc.name)
				if !ok {
					t.Fatalf("plugins toolbar button %q missing", tc.name)
				}
				if info.Bounds.H != 28 || (tc.square && info.Bounds.W != 28) {
					t.Fatalf("plugins toolbar button %q bounds=%+v, want h=28 square=%v", tc.name, info.Bounds, tc.square)
				}
				px := listIm.RGBAAt(int(info.Bounds.X+2), int(info.Bounds.Y+2))
				diff := absBoardUIChannel(int(px.R)-int(pluginsBarBg.R)) + absBoardUIChannel(int(px.G)-int(pluginsBarBg.G)) + absBoardUIChannel(int(px.B)-int(pluginsBarBg.B))
				if diff > 12 {
					t.Fatalf("plugins toolbar button %q corner pixel=%v differs from toolbar bg=%v (diff=%d)", tc.name, px, pluginsBarBg, diff)
				}
			}

			rowButtons := []string{
				"unic.wenshushu",
				"卸载文叔叔临时分享",
				"unic.syncclipboard",
				"卸载SyncClipboard 同步",
				"mkdir700.wxime",
				"卸载微信输入法剪贴板",
				"Tailcat 同步",
				"文本工具",
				"内容分类",
			}
			for _, name := range rowButtons {
				info, ok := tt.AccessByRole(ui.RoleButton, name)
				if !ok {
					t.Fatalf("plugins row button %q missing", name)
				}
				if info.Bounds.W != 28 || info.Bounds.H != 28 {
					t.Fatalf("plugins row button %q bounds=%+v, want 28x28", name, info.Bounds)
				}
				// Sample 4 DIP above/left of the button inside the same settings card row.
				cardBg := listIm.RGBAAt(int(info.Bounds.X-4), int(info.Bounds.Y+info.Bounds.H/2))
				px := listIm.RGBAAt(int(info.Bounds.X+2), int(info.Bounds.Y+2))
				diff := absBoardUIChannel(int(px.R)-int(cardBg.R)) + absBoardUIChannel(int(px.G)-int(cardBg.G)) + absBoardUIChannel(int(px.B)-int(cardBg.B))
				if diff > 12 {
					t.Fatalf("plugins row button %q corner pixel=%v differs from row card bg=%v (diff=%d); expected transparent rest surface", name, px, cardBg, diff)
				}
			}
		})
	}
}

func TestPluginAsyncInstallCancelAndConfigurableToast(t *testing.T) {
	dir := t.TempDir()
	sock := filepath.Join(dir, "unic.sock")
	ln, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	var mu sync.Mutex
	installed := []api.PluginInfo{}
	installStarted := make(chan string, 4)
	installCanceled := make(chan string, 4)
	allowFinish := make(chan struct{}, 4)

	const (
		syncRepo  = "https://github.com/UniClipboard/unic-plugin-syncclipboard"
		shareRepo = "https://github.com/UniClipboard/unic-plugin-wenshushu"
	)

	syncSchema := json.RawMessage(`{"$schema":"https://json-schema.org/draft/2020-12/schema","x-unic-settings-version":1,"type":"object","additionalProperties":false,"properties":{"url":{"type":"string","title":"服务器地址"}},"required":["url"]}`)

	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.Status{ID: "svc-1", Name: "Test Mac", Peers: []api.PeerStatus{}, Pending: []api.PendingPeer{}})
	})
	mux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(config.State{Revision: "rev-1", Config: config.Default()})
	})
	mux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		out := append([]api.PluginInfo(nil), installed...)
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(out)
	})
	mux.HandleFunc("GET /v1/plugins/{id}/settings", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.PluginSettings{
			Revision: "s-rev-1",
			Schema:   syncSchema,
			Values:   json.RawMessage(`{"url":"https://sync.example.com"}`),
		})
	})
	mux.HandleFunc("GET /v1/plugins/marketplace", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.PluginMarketplace{
			Version: 1,
			Source:  "remote",
			Plugins: []api.MarketplaceEntry{
				{
					ID:           "unic.syncclipboard",
					Name:         "SyncClipboard 同步",
					Author:       "UniClipboard 官方",
					Official:     true,
					Category:     "sync",
					Version:      "0.2.0",
					Summary:      "可配置的同步扩展。",
					Description:  "支持配置服务器地址与凭据。",
					Repository:   syncRepo,
					Capabilities: []string{"sync_backend"},
				},
				{
					ID:           "unic.wenshushu",
					Name:         "文叔叔临时分享",
					Author:       "UniClipboard 官方",
					Official:     true,
					Category:     "share",
					Version:      "0.1.0",
					Summary:      "免配置临时分享。",
					Description:  "无需配置，开箱即用。",
					ZeroConfig:   true,
					Repository:   shareRepo,
					Capabilities: []string{"action"},
				},
			},
		})
	})
	mux.HandleFunc("POST /v1/plugins/install", func(w http.ResponseWriter, r *http.Request) {
		var req api.PluginInstallRequest
		_ = json.NewDecoder(r.Body).Decode(&req)
		installStarted <- req.Repository
		select {
		case <-r.Context().Done():
			installCanceled <- req.Repository
			return
		case <-allowFinish:
		}
		var info api.PluginInfo
		if req.Repository == syncRepo {
			info = api.PluginInfo{
				ID:               "unic.syncclipboard",
				ImplementationID: "unic.syncclipboard",
				Version:          "0.2.0",
				Kind:             "go",
				Source:           "prebuilt",
				Repository:       syncRepo,
				Capabilities:     []string{"sync_backend"},
				HasSettings:      true,
				LiveStart:        true,
			}
		} else {
			info = api.PluginInfo{
				ID:               "unic.wenshushu",
				ImplementationID: "unic.wenshushu",
				Version:          "0.1.0",
				Kind:             "go",
				Source:           "prebuilt",
				Repository:       shareRepo,
				Capabilities:     []string{"action"},
				HasSettings:      false,
				LiveStart:        true,
			}
		}
		mu.Lock()
		installed = append(installed, info)
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(info)
	})
	mux.HandleFunc("GET /v1/actions", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.Action{}) })
	mux.HandleFunc("GET /v1/sync-backends", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.SyncBackendInfo{}) })
	mux.HandleFunc("GET /v1/history", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode(api.HistoryPage{}) })
	mux.HandleFunc("POST /v1/history/search", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode(api.HistorySearchPage{}) })
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
	d.pluginSettings.view = "marketplace"
	d.pluginSettings.marketSelected = "unic.syncclipboard"

	tt := ui.NewTesterWith(desktop.View, 1120, 860, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
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
		t.Fatalf("timeout waiting for %s; texts=%v", label, tt.Texts())
	}

	wait("marketplace loaded", func() bool {
		return d.live.loaded && d.live.marketplaceLoaded && tt.HasText("SyncClipboard 同步") && tt.HasText("安装")
	})

	// 1. Start installing SyncClipboard; page must remain unblocked (can switch cards and navigate back).
	if err := tt.Click("安装"); err != nil {
		t.Fatalf("click 安装: %v", err)
	}
	select {
	case repo := <-installStarted:
		if repo != syncRepo {
			t.Fatalf("unexpected repo started: %s", repo)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("install did not start")
	}
	wait("installing state and progress toast", func() bool {
		return !d.live.busy && tt.HasText("正在安装…") && tt.HasText("正在安装「SyncClipboard 同步」…") && tt.HasText("取消安装")
	})

	// While installing, switch to another marketplace plugin card and verify it is interactive.
	if err := tt.Click("文叔叔临时分享"); err != nil {
		t.Fatalf("switch card during install: %v", err)
	}
	wait("switched to Wenshushu while SyncClipboard installs", func() bool {
		return d.pluginSettings.marketSelected == "unic.wenshushu" && tt.HasText("无需配置，开箱即用。")
	})
	// Switch back to SyncClipboard and cancel from the in-page cancel button.
	if err := tt.Click("SyncClipboard 同步"); err != nil {
		t.Fatalf("switch back to SyncClipboard: %v", err)
	}
	wait("SyncClipboard detail visible with cancel button", func() bool {
		return d.pluginSettings.marketSelected == "unic.syncclipboard" && tt.HasText("正在安装…")
	})
	if err := tt.Click("取消安装"); err != nil {
		t.Fatalf("click in-page 取消安装: %v", err)
	}
	select {
	case repo := <-installCanceled:
		if repo != syncRepo {
			t.Fatalf("unexpected canceled repo: %s", repo)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("server request context was not canceled")
	}
	wait("canceled toast shown and install button restored", func() bool {
		return d.pluginInstallTask("unic.syncclipboard") == nil && tt.HasText("已取消安装「SyncClipboard 同步」") && tt.HasText("安装")
	})

	// 2. Start installing again, navigate away to History, and cancel via the persistent progress toast.
	if err := tt.Click("安装"); err != nil {
		t.Fatalf("click 安装 second time: %v", err)
	}
	select {
	case <-installStarted:
	case <-time.After(3 * time.Second):
		t.Fatal("second install did not start")
	}
	wait("second install active", func() bool {
		return tt.HasText("正在安装「SyncClipboard 同步」…")
	})
	if err := tt.Click("返回历史"); err != nil {
		t.Fatalf("navigate to history during install: %v", err)
	}
	wait("on history page with progress toast", func() bool {
		return d.page == "history" && tt.HasText("正在安装「SyncClipboard 同步」…") && tt.HasText("取消安装")
	})
	if err := tt.Click("取消安装"); err != nil {
		t.Fatalf("cancel install from toast on history page: %v", err)
	}
	select {
	case <-installCanceled:
	case <-time.After(3 * time.Second):
		t.Fatal("toast cancel did not cancel server request")
	}
	wait("toast cancel completed", func() bool {
		return d.pluginInstallTask("unic.syncclipboard") == nil && tt.HasText("已取消安装「SyncClipboard 同步」")
	})

	// 3. Start installing configurable plugin, stay on History, let install finish, and click 前往配置 in the completion toast.
	d.page = "settings"
	d.settings.Value = "plugins"
	d.pluginSettings.view = "marketplace"
	d.pluginSettings.marketSelected = "unic.syncclipboard"
	tt.Frame()
	if err := tt.Click("安装"); err != nil {
		t.Fatalf("click 安装 third time: %v", err)
	}
	select {
	case <-installStarted:
	case <-time.After(3 * time.Second):
		t.Fatal("third install did not start")
	}
	if err := tt.Click("返回历史"); err != nil {
		t.Fatalf("return to history: %v", err)
	}
	wait("on history while installing", func() bool { return d.page == "history" })
	allowFinish <- struct{}{}
	wait("completion toast with configure entry on history page", func() bool {
		return d.pluginInstallTask("unic.syncclipboard") == nil &&
			tt.HasText("已安装「SyncClipboard 同步」。") &&
			tt.HasText("前往配置")
	})
	if err := tt.Click("前往配置"); err != nil {
		t.Fatalf("click 前往配置 in toast: %v", err)
	}
	wait("navigated to plugin configuration page", func() bool {
		return d.page == "settings" && d.settings.Value == "plugins" &&
			d.pluginSettings.id == "unic.syncclipboard" && tt.HasText("插件设置") && tt.HasText("服务器地址")
	})

	// 4. Verify zero-config plugin completion toast omits 前往配置.
	d.pluginSettings.id = ""
	d.pluginSettings.view = "marketplace"
	d.pluginSettings.marketSelected = "unic.wenshushu"
	tt.Frame()
	if err := tt.Click("安装"); err != nil {
		t.Fatalf("click 安装 for wenshushu: %v", err)
	}
	select {
	case <-installStarted:
	case <-time.After(3 * time.Second):
		t.Fatal("wenshushu install did not start")
	}
	allowFinish <- struct{}{}
	wait("wenshushu completion toast without configure action", func() bool {
		return d.pluginInstallTask("unic.wenshushu") == nil && tt.HasText("已安装「文叔叔临时分享」。")
	})
	if tt.HasText("前往配置") {
		t.Fatal("zero-config plugin toast unexpectedly showed 前往配置")
	}
}

// TestCaptureAsyncPluginInstallRecording renders a 20 FPS frame sequence
// demonstrating non-blocking async plugin installation, cross-page cancellation,
// configurable completion toast ("前往配置"), and zero-config completion toast.
// Opt-in via UNIC_PLUGIN_INSTALL_RECORD_DIR.
func TestCaptureAsyncPluginInstallRecording(t *testing.T) {
	outDir := os.Getenv("UNIC_PLUGIN_INSTALL_RECORD_DIR")
	if outDir == "" {
		t.Skip("set UNIC_PLUGIN_INSTALL_RECORD_DIR to record async plugin install evidence frames")
	}
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		t.Fatal(err)
	}

	dir := t.TempDir()
	sock := filepath.Join(dir, "unic.sock")
	ln, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	var mu sync.Mutex
	installed := []api.PluginInfo{}
	installStarted := make(chan string, 4)
	installCanceled := make(chan string, 4)
	allowFinish := make(chan struct{}, 4)

	const (
		syncRepo  = "https://github.com/UniClipboard/unic-plugin-syncclipboard"
		shareRepo = "https://github.com/UniClipboard/unic-plugin-wenshushu"
	)
	syncSchema := json.RawMessage(`{"$schema":"https://json-schema.org/draft/2020-12/schema","x-unic-settings-version":1,"type":"object","additionalProperties":false,"properties":{"url":{"type":"string","title":"服务器地址","description":"SyncClipboard 兼容服务地址"},"token":{"type":"string","title":"访问令牌","writeOnly":true}},"required":["url"]}`)

	nowMs := time.Now().UnixMilli()
	sampleItems := []api.HistorySearchHit{
		{
			Item: api.HistoryItem{
				ID:         "h-1",
				Revision:   1,
				Kind:       "text",
				Origin:     "local_system",
				CreatedAt:  nowMs - 120_000,
				TotalBytes: 96,
				Tags:       []string{"builtin:text"},
				Parts:      []api.HistoryPart{{Available: true, Kind: "representation", MIME: "text/plain", Size: 96}},
			},
			Preview: engine.HistorySearchPreview{Text: "UniClipboard 异步插件安装方案评审纪要"},
		},
		{
			Item: api.HistoryItem{
				ID:         "h-2",
				Revision:   1,
				Kind:       "text",
				Origin:     "local_system",
				CreatedAt:  nowMs - 600_000,
				TotalBytes: 44,
				Tags:       []string{"builtin:text", "builtin:link"},
				Parts:      []api.HistoryPart{{Available: true, Kind: "representation", MIME: "text/plain", Size: 44}},
			},
			Preview: engine.HistorySearchPreview{Text: "https://github.com/UniClipboard/unic-plugins"},
		},
	}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.Status{ID: "svc-1", Name: "MacBook Pro", Peers: []api.PeerStatus{}, Pending: []api.PendingPeer{}})
	})
	mux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(config.State{Revision: "rev-1", Config: config.Default()})
	})
	mux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		out := append([]api.PluginInfo(nil), installed...)
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(out)
	})
	mux.HandleFunc("GET /v1/plugins/{id}/settings", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.PluginSettings{
			Revision: "s-rev-1",
			Schema:   syncSchema,
			Values:   json.RawMessage(`{"url":"https://sync.example.com"}`),
		})
	})
	mux.HandleFunc("GET /v1/plugins/marketplace", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.PluginMarketplace{
			Version: 1,
			Source:  "remote",
			Plugins: []api.MarketplaceEntry{
				{
					ID:           "unic.syncclipboard",
					Name:         "SyncClipboard 同步",
					Author:       "UniClipboard 官方",
					Official:     true,
					Category:     "sync",
					Version:      "0.2.0",
					Summary:      "通过 SyncClipboard 客户端双向同步文字和图片。",
					Description:  "为兼容客户端提供文字和图片同步，安装后需配置服务地址与访问令牌。",
					Highlights:   []string{"独立同步插件", "支持配置服务地址与令牌"},
					HowTo:        []string{"安装插件后点击「前往配置」", "填写服务地址与令牌并启用"},
					Privacy:      "凭据仅加密保存在本地配置文件。",
					Repository:   syncRepo,
					Capabilities: []string{"sync_backend"},
				},
				{
					ID:           "unic.wenshushu",
					Name:         "文叔叔临时分享",
					Author:       "UniClipboard 官方",
					Official:     true,
					Category:     "share",
					Version:      "0.1.1",
					Summary:      "一键生成 24 小时有效的临时下载链接。",
					Description:  "无需配置，选中任意剪贴板条目即可一键分享。",
					Highlights:   []string{"免配置开箱即用", "支持文本、图片与文件"},
					HowTo:        []string{"选中任意剪贴板条目", "点击动作菜单中的「通过文叔叔临时分享」"},
					Privacy:      "仅在主动触发动作时上传所选内容。",
					ZeroConfig:   true,
					Repository:   shareRepo,
					Capabilities: []string{"action"},
				},
			},
		})
	})
	mux.HandleFunc("POST /v1/plugins/install", func(w http.ResponseWriter, r *http.Request) {
		var req api.PluginInstallRequest
		_ = json.NewDecoder(r.Body).Decode(&req)
		installStarted <- req.Repository
		select {
		case <-r.Context().Done():
			installCanceled <- req.Repository
			return
		case <-allowFinish:
		}
		var info api.PluginInfo
		if req.Repository == syncRepo {
			info = api.PluginInfo{
				ID:               "unic.syncclipboard",
				ImplementationID: "unic.syncclipboard",
				Name:             "SyncClipboard 同步",
				Version:          "0.2.0",
				Kind:             "go",
				Source:           "prebuilt",
				Repository:       syncRepo,
				Capabilities:     []string{"sync_backend"},
				HasSettings:      true,
				LiveStart:        true,
			}
		} else {
			info = api.PluginInfo{
				ID:               "unic.wenshushu",
				ImplementationID: "unic.wenshushu",
				Name:             "文叔叔临时分享",
				Version:          "0.1.1",
				Kind:             "go",
				Source:           "prebuilt",
				Repository:       shareRepo,
				Capabilities:     []string{"action"},
				HasSettings:      false,
				LiveStart:        true,
			}
		}
		mu.Lock()
		installed = append(installed, info)
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(info)
	})
	mux.HandleFunc("GET /v1/actions", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.Action{}) })
	mux.HandleFunc("GET /v1/sync-backends", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.SyncBackendInfo{}) })
	mux.HandleFunc("GET /v1/history", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.HistoryPage{Items: []api.HistoryItem{sampleItems[0].Item, sampleItems[1].Item}})
	})
	mux.HandleFunc("POST /v1/history/search", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.HistorySearchPage{Hits: sampleItems})
	})
	mux.HandleFunc("GET /v1/history/{id}/content", func(w http.ResponseWriter, r *http.Request) {
		switch r.PathValue("id") {
		case "h-1":
			_, _ = w.Write([]byte("UniClipboard 异步插件安装方案评审纪要\n1. 插件安装异步执行，不阻塞当前页面\n2. 安装完成后通过 Toast 通知，可配置插件提供「前往配置」入口\n3. 安装过程中支持在页内或 Toast 随时取消"))
		default:
			_, _ = w.Write([]byte("https://github.com/UniClipboard/unic-plugins"))
		}
	})
	mux.HandleFunc("GET /v1/history/tags", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode([]string{"builtin:text", "builtin:link"})
	})
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
	d.pluginSettings.view = "marketplace"
	d.pluginSettings.marketSelected = "unic.syncclipboard"

	tt := ui.NewTesterWith(desktop.View, 1100, 720, ui.TesterOptions{})
	tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 78})

	wait := func(label string, cond func() bool) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			d.live.poll(d)
			tt.Advance(20 * time.Millisecond)
			if cond() {
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		t.Fatalf("timeout waiting for %s", label)
	}
	wait("initial load", func() bool {
		return d.live.loaded && d.live.marketplaceLoaded && tt.HasText("SyncClipboard 同步") && tt.HasText("安装")
	})
	tt.Advance(300 * time.Millisecond)

	const frameStep = 50 * time.Millisecond // 20 FPS
	frameIdx := 0
	curX, curY := float32(550), float32(360)

	drawCursor := func(dst *image.RGBA, x, y float32, ripple float32) {
		b := dst.Bounds()
		blend := func(px, py int, c color.RGBA) {
			if px < b.Min.X || px >= b.Max.X || py < b.Min.Y || py >= b.Max.Y {
				return
			}
			bg := dst.RGBAAt(px, py)
			a := float64(c.A) / 255.0
			dst.SetRGBA(px, py, color.RGBA{
				R: uint8(float64(c.R)*a + float64(bg.R)*(1-a)),
				G: uint8(float64(c.G)*a + float64(bg.G)*(1-a)),
				B: uint8(float64(c.B)*a + float64(bg.B)*(1-a)),
				A: 255,
			})
		}
		if ripple > 0 {
			r := 8 + ripple*16
			alpha := uint8(max(0, (1-ripple)*180))
			for dy := -int(r) - 2; dy <= int(r)+2; dy++ {
				for dx := -int(r) - 2; dx <= int(r)+2; dx++ {
					dist := math.Hypot(float64(dx), float64(dy))
					if math.Abs(dist-float64(r)) <= 1.6 {
						blend(int(x)+dx, int(y)+dy, color.RGBA{R: 37, G: 99, B: 235, A: alpha})
					}
				}
			}
		}
		// Soft circular pointer dot with white halo + dark border for high contrast on any surface.
		for dy := -9; dy <= 9; dy++ {
			for dx := -9; dx <= 9; dx++ {
				dist := math.Hypot(float64(dx), float64(dy))
				if dist <= 5.5 {
					blend(int(x)+dx, int(y)+dy, color.RGBA{R: 29, G: 78, B: 216, A: 235})
				} else if dist <= 7.5 {
					blend(int(x)+dx, int(y)+dy, color.RGBA{R: 255, G: 255, B: 255, A: 245})
				} else if dist <= 8.5 {
					blend(int(x)+dx, int(y)+dy, color.RGBA{R: 15, G: 23, B: 42, A: 90})
				}
			}
		}
	}

	snap := func(ripple float32) {
		d.live.poll(d)
		src := tt.Image()
		dst := image.NewRGBA(src.Bounds())
		draw.Draw(dst, dst.Bounds(), src, src.Bounds().Min, draw.Src)
		drawCursor(dst, curX, curY, ripple)
		f, err := os.Create(filepath.Join(outDir, fmt.Sprintf("frame-%04d.png", frameIdx)))
		if err != nil {
			t.Fatal(err)
		}
		if err := png.Encode(f, dst); err != nil {
			_ = f.Close()
			t.Fatal(err)
		}
		_ = f.Close()
		frameIdx++
	}

	hold := func(frames int) {
		for range frames {
			tt.Advance(frameStep)
			snap(0)
		}
	}

	moveAndClick := func(target string) {
		t.Helper()
		var rect ui.Rect
		if info, ok := tt.AccessByRole(ui.RoleButton, target); ok {
			rect = info.Bounds
		} else if info, ok := tt.Access(target); ok {
			rect = info.Bounds
		} else if r, ok := tt.Find(target); ok {
			rect = r
		} else {
			t.Fatalf("recording target %q not found; texts=%v", target, tt.Texts())
		}
		tx, ty := rect.X+rect.W/2, rect.Y+rect.H/2
		const steps = 8
		sx, sy := curX, curY
		for i := 1; i <= steps; i++ {
			p := float32(i) / steps
			ease := 1 - (1-p)*(1-p)
			curX = sx + (tx-sx)*ease
			curY = sy + (ty-sy)*ease
			tt.Move(curX, curY)
			tt.Advance(frameStep)
			snap(0)
		}
		tt.Press(tx, ty)
		tt.Advance(frameStep)
		snap(0.25)
		tt.Release(tx, ty)
		tt.Advance(frameStep)
		snap(0.6)
		tt.Advance(frameStep)
		snap(0.95)
	}

	// Scene 1: Marketplace overview (0.7s)
	hold(14)

	// Scene 2: Click "安装" on SyncClipboard -> async install starts, page stays completely interactive
	moveAndClick("安装")
	<-installStarted
	hold(16)

	// Prove non-blocking: switch to Wenshushu card while SyncClipboard installs in background
	moveAndClick("文叔叔临时分享")
	hold(18)

	// Navigate away to History while SyncClipboard is still installing -> persistent progress toast stays visible
	moveAndClick("返回历史")
	hold(20)

	// Scene 3: Cancel installation directly from the toast on History page
	moveAndClick("取消安装")
	<-installCanceled
	wait("canceled toast", func() bool {
		return d.pluginInstallTask("unic.syncclipboard") == nil && tt.HasText("已取消安装「SyncClipboard 同步」")
	})
	hold(26)

	// Scene 4: Navigate back to Settings -> Plugins (returns directly to Marketplace),
	// install SyncClipboard, return to History, complete -> click "前往配置".
	moveAndClick("设置…")
	wait("settings opened", func() bool { return d.page == "settings" && tt.HasText("插件") })
	hold(6)
	moveAndClick("插件")
	wait("marketplace reopened", func() bool {
		return d.settings.Value == "plugins" && d.pluginSettings.view == "marketplace" && tt.HasText("SyncClipboard 同步")
	})
	moveAndClick("SyncClipboard 同步")
	wait("syncclipboard selected", func() bool { return d.pluginSettings.marketSelected == "unic.syncclipboard" && tt.HasText("安装") })
	hold(8)

	moveAndClick("安装")
	<-installStarted
	hold(12)

	moveAndClick("返回历史")
	hold(18)

	// Complete SyncClipboard installation while user is browsing History
	allowFinish <- struct{}{}
	wait("syncclipboard completion toast", func() bool {
		return d.pluginInstallTask("unic.syncclipboard") == nil && tt.HasText("前往配置")
	})
	hold(24)

	// Click "前往配置" in the toast -> jumps directly to SyncClipboard settings page
	moveAndClick("前往配置")
	wait("syncclipboard settings loaded", func() bool {
		return d.page == "settings" && d.pluginSettings.id == "unic.syncclipboard" && tt.HasText("服务器地址")
	})
	hold(28)

	// Scene 5: Contrast with zero-config plugin (Wenshushu) -> completion toast has no "前往配置"
	moveAndClick("返回插件列表")
	wait("plugins list", func() bool { return d.pluginSettings.id == "" && tt.HasText("浏览插件市场") })
	hold(6)
	moveAndClick("浏览插件市场")
	wait("marketplace for wenshushu", func() bool { return d.pluginSettings.view == "marketplace" && tt.HasText("文叔叔临时分享") })
	moveAndClick("文叔叔临时分享")
	wait("wenshushu detail selected", func() bool { return d.pluginSettings.marketSelected == "unic.wenshushu" && tt.HasText("安装") })
	hold(8)

	moveAndClick("安装")
	<-installStarted
	hold(16)

	allowFinish <- struct{}{}
	wait("wenshushu completion toast", func() bool {
		return d.pluginInstallTask("unic.wenshushu") == nil && tt.HasText("已安装「文叔叔临时分享」。")
	})
	hold(30)

	t.Logf("captured %d frames into %s", frameIdx, outDir)
}
