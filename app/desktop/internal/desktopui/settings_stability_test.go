package desktopui

import (
	"context"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/BurntSushi/toml"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestSettingsAndPluginsAvoidFastBusyFlicker(t *testing.T) {
	dir, err := os.MkdirTemp("/tmp", "unic-set-stable-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = os.RemoveAll(dir) }()
	sock := filepath.Join(dir, "s.sock")
	ln, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	var mu sync.Mutex
	curCfg := config.Default()
	rev := 1
	var saveGate chan struct{}
	var saveCalls atomic.Int32

	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.Status{ID: "local-id", Name: "MacBook Pro"})
	})
	mux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		c := curCfg
		rStr := string(rune('0' + rev))
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(config.State{Revision: "rev-" + rStr, Config: c})
	})
	mux.HandleFunc("PUT /v1/config", func(w http.ResponseWriter, r *http.Request) {
		saveCalls.Add(1)
		raw, _ := io.ReadAll(r.Body)
		mu.Lock()
		gate := saveGate
		mu.Unlock()
		if gate != nil {
			<-gate
		}
		var next config.Config
		if err := toml.Unmarshal(raw, &next); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		mu.Lock()
		curCfg = next
		rev++
		rStr := string(rune('0' + rev))
		out := config.State{Revision: "rev-" + rStr, Config: curCfg}
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(out)
	})
	mux.HandleFunc("GET /v1/actions", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"actions": []api.Action{}})
	})
	mux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode([]api.PluginInfo{})
	})
	mux.HandleFunc("GET /v1/sync-backends", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode([]api.SyncBackendInfo{
			{ID: "builtin.tailcat", ImplementationID: "builtin.tailcat", TransportKind: "peer_sessions", State: "running", Pairing: true},
		})
	})
	mux.HandleFunc("GET /v1/history/tags", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode([]string{})
	})
	mux.HandleFunc("GET /v1/candidates", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode([]api.Candidate{})
	})
	mux.HandleFunc("GET /v1/events", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		<-r.Context().Done()
	})
	mux.HandleFunc("POST /v1/history/search/counts", func(w http.ResponseWriter, r *http.Request) {
		var req engine.HistorySearchCountsRequest
		_ = json.NewDecoder(r.Body).Decode(&req)
		_ = json.NewEncoder(w).Encode(map[string]any{"counts": make([]uint64, len(req.Queries))})
	})
	mux.HandleFunc("POST /v1/history/search/candidates", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"candidates": []any{}})
	})
	mux.HandleFunc("GET /v1/history", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.HistoryPage{Items: []api.HistoryItem{}, Revision: 1})
	})
	mux.HandleFunc("POST /v1/history/search", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.HistorySearchPage{Hits: []api.HistorySearchHit{}, HistoryRevision: 1})
	})

	srv := &http.Server{Handler: mux}
	go func() { _ = srv.Serve(ln) }()
	defer func() { _ = srv.Close() }()

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	desktop := NewDesktop(ctx, api.NewClient(sock))
	defer desktop.Close()
	d := desktop.d
	live := d.live
	d.page = "settings"
	d.settings.Value = "sync"

	tt := ui.NewTesterWith(d.view, 1120, 760, ui.TesterOptions{})
	waitFor := func(desc string, cond func() bool) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			tt.Frame()
			if cond() {
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		t.Fatalf("timed out waiting for %s", desc)
	}

	waitFor("initial settings loaded", func() bool {
		return live.loaded && !live.loading
	})
	tt.Frame()

	clickSwitch := func(label string) {
		t.Helper()
		n, ok := tt.AccessByRole(ui.RoleSwitch, label)
		if !ok {
			t.Fatalf("missing switch %q", label)
		}
		tt.ClickAt(n.Bounds.X+n.Bounds.W/2, n.Bounds.Y+n.Bounds.H/2)
	}

	// 1. Settings > Sync: toggling "自动同步" must not flash sibling switches/controls during <120ms save.
	baselineImg := cloneRGBA(tt.Image())
	siblingBefore, ok := tt.AccessByRole(ui.RoleSwitch, "使用历史时同步")
	if !ok {
		t.Fatal("missing sibling switch 使用历史时同步")
	}
	filesBefore, ok := tt.AccessByRole(ui.RoleSwitch, "同步文件")
	if !ok {
		t.Fatal("missing sibling switch 同步文件")
	}

	mu.Lock()
	saveGate = make(chan struct{})
	mu.Unlock()

	clickSwitch("自动同步")
	live.busySince = time.Now().Add(5 * time.Second)
	tt.Frame()
	inflightImg := cloneRGBA(tt.Image())

	if !live.busy || live.showBusy() {
		t.Fatalf("expected fast busy window, got busy=%v showBusy=%v", live.busy, live.showBusy())
	}
	autoSw, _ := tt.AccessByRole(ui.RoleSwitch, "自动同步")
	if autoSw.Checked || autoSw.Disabled {
		t.Fatalf("toggled switch bounced or disabled in-flight: checked=%v disabled=%v", autoSw.Checked, autoSw.Disabled)
	}
	for _, pair := range []struct {
		label  string
		before ui.Rect
	}{
		{"使用历史时同步", siblingBefore.Bounds},
		{"同步文件", filesBefore.Bounds},
	} {
		n, ok := tt.AccessByRole(ui.RoleSwitch, pair.label)
		if !ok || n.Disabled || n.Bounds != pair.before {
			t.Fatalf("sibling switch %q flickered or moved: ok=%v disabled=%v bounds=%+v want=%+v", pair.label, ok, n.Disabled, n.Bounds, pair.before)
		}
		if !regionPixelsEqual(baselineImg, inflightImg, pair.before) {
			t.Fatalf("sibling switch %q pixels flickered during <120ms config save", pair.label)
		}
	}

	// Concurrent toggle of sibling while busy must be ignored.
	clickSwitch("同步文件")
	if !live.config.Config.Sync.Files {
		t.Fatal("concurrent sibling switch click bypassed busy guard")
	}

	close(saveGate)
	waitFor("sync config save completed", func() bool { return !live.busy })
	if saveCalls.Load() != 1 {
		t.Fatalf("expected 1 PUT /v1/config call, got %d", saveCalls.Load())
	}

	// 2. Settings > Plugins: toggling a built-in plugin switch must not flash unrelated "浏览插件市场" button.
	d.settings.Value = "plugins"
	d.live.marketplaceLoaded = true
	tt.Frame()

	marketBefore, ok := tt.AccessByRole(ui.RoleButton, "浏览插件市场")
	if !ok {
		t.Fatal("missing 浏览插件市场 button")
	}
	pluginsBaseline := cloneRGBA(tt.Image())

	mu.Lock()
	saveGate = make(chan struct{})
	mu.Unlock()

	clickSwitch("启用文本工具")
	live.busySince = time.Now().Add(5 * time.Second)
	tt.Frame()
	pluginsInflight := cloneRGBA(tt.Image())

	marketInflight, ok := tt.AccessByRole(ui.RoleButton, "浏览插件市场")
	if !ok || marketInflight.Disabled || marketInflight.Bounds != marketBefore.Bounds {
		t.Fatalf("浏览插件市场 button flickered during plugin switch: ok=%v disabled=%v bounds=%+v", ok, marketInflight.Disabled, marketInflight.Bounds)
	}
	if !regionPixelsEqual(pluginsBaseline, pluginsInflight, marketBefore.Bounds) {
		t.Fatal("浏览插件市场 button pixels flickered during <120ms plugin toggle")
	}
	close(saveGate)
	waitFor("plugin config save completed", func() bool { return !live.busy })
}
