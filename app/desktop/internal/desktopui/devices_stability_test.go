package desktopui

import (
	"context"
	"encoding/json"
	"image"
	"math"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func regionPixelsEqual(a, b *image.RGBA, r ui.Rect) bool {
	bounds := a.Bounds().Intersect(b.Bounds())
	minX := max(bounds.Min.X, int(math.Floor(float64(r.X))))
	minY := max(bounds.Min.Y, int(math.Floor(float64(r.Y))))
	maxX := min(bounds.Max.X, int(math.Ceil(float64(r.X+r.W))))
	maxY := min(bounds.Max.Y, int(math.Ceil(float64(r.Y+r.H))))
	for y := minY; y < maxY; y++ {
		for x := minX; x < maxX; x++ {
			if a.RGBAAt(x, y) != b.RGBAAt(x, y) {
				return false
			}
		}
	}
	return true
}

func TestDeviceSyncPolicySwitchAvoidsWholePageFlickerAndStaleGap(t *testing.T) {
	dir, err := os.MkdirTemp("/tmp", "unic-dev-stable-")
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
	binding := engine.DeviceBinding{
		ID:             "b-1",
		InstanceID:     "builtin.tailcat",
		State:          "authorized",
		SyncEnabled:    true,
		Policy:         engine.DefaultBindingSyncPolicy(),
		Connection:     "connected",
		ConnectionPath: "direct",
		LatencyMs:      8,
		Revision:       1,
		LastActiveMs:   time.Now().Add(-2 * time.Minute).UnixMilli(),
		LastCheckedMs:  time.Now().Add(-10 * time.Second).UnixMilli(),
	}
	var patchGate chan struct{}
	var statusGate chan struct{}
	var failNextPatch bool
	var patchCalls atomic.Int32

	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		gate := statusGate
		mu.Unlock()
		if gate != nil {
			<-gate
		}
		mu.Lock()
		b := binding
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(api.Status{
			ID:   "local-id",
			Name: "MacBook Pro",
			Peers: []api.PeerStatus{
				{
					ID:           "peer-1",
					Name:         "Mac mini",
					ReportedName: "Mac mini",
					Connected:    true,
					LastSeen:     time.Now().Add(-2 * time.Minute).Unix(),
					Fingerprint:  "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D",
					Revision:     1,
					Bindings:     []engine.DeviceBinding{b},
				},
			},
		})
	})
	mux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(config.State{Revision: "rev-1", Config: config.Default()})
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
	mux.HandleFunc("PATCH /v1/bindings/{id}", func(w http.ResponseWriter, r *http.Request) {
		patchCalls.Add(1)
		var edit api.BindingEdit
		_ = json.NewDecoder(r.Body).Decode(&edit)
		mu.Lock()
		gate := patchGate
		fail := failNextPatch
		failNextPatch = false
		mu.Unlock()
		if gate != nil {
			<-gate
		}
		if fail {
			w.WriteHeader(http.StatusConflict)
			_ = json.NewEncoder(w).Encode(api.Error{Reason: "conflict", Message: "stale revision"})
			return
		}
		mu.Lock()
		if edit.Enabled != nil {
			binding.SyncEnabled = *edit.Enabled
		}
		if edit.Policy != nil {
			binding.Policy = edit.Policy.Normalized()
		}
		binding.Revision++
		out := binding
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(out)
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
	d.page = "devices"
	d.deviceList.Value = "peer-1"

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

	waitFor("initial devices loaded", func() bool {
		return live.loaded && !live.loading && d.deviceByID("peer-1") != nil
	})
	tt.Frame()

	unrelatedControls := []struct {
		role  ui.Role
		label string
	}{
		{ui.RoleButton, "刷新状态"},
		{ui.RoleButton, "检测连接"},
		{ui.RoleSwitch, "同步 Tailcat 同步"},
		{ui.RoleButton, "移除设备"},
	}

	baselineImg := cloneRGBA(tt.Image())
	baselineBounds := map[string]ui.Rect{}
	for _, c := range unrelatedControls {
		n, ok := tt.AccessByRole(c.role, c.label)
		if !ok {
			t.Fatalf("missing control %q before switch", c.label)
		}
		baselineBounds[c.label] = n.Bounds
	}

	// 1. Click quick preset "仅发送" while PATCH and Status are gated.
	mu.Lock()
	patchGate = make(chan struct{})
	statusGate = make(chan struct{})
	mu.Unlock()

	if err := tt.Click("仅发送"); err != nil {
		t.Fatal(err)
	}
	// Pin busySince slightly ahead so slow -race software frames stay deterministically within <120ms.
	live.busySince = time.Now().Add(5 * time.Second)
	tt.Frame()
	inflightImg := cloneRGBA(tt.Image())

	if !live.busy || live.showBusy() {
		t.Fatalf("expected fast busy window (busy=true, showBusy=false), got busy=%v showBusy=%v", live.busy, live.showBusy())
	}
	wantSendOnly := engine.BindingSyncPolicy{
		Text:   engine.SyncDirectionSendOnly,
		Images: engine.SyncDirectionSendOnly,
		Files:  engine.SyncDirectionSendOnly,
	}
	if got := d.deviceByID("peer-1").bindings[0].policy; got != wantSendOnly {
		t.Fatalf("policy during in-flight = %+v, want immediate %+v", got, wantSendOnly)
	}
	for _, c := range unrelatedControls {
		n, ok := tt.AccessByRole(c.role, c.label)
		if !ok {
			t.Fatalf("control %q disappeared during fast in-flight switch", c.label)
		}
		if n.Disabled {
			t.Fatalf("unrelated control %q flashed disabled during <120ms policy switch", c.label)
		}
		if n.Bounds != baselineBounds[c.label] {
			t.Fatalf("control %q shifted bounds during switch: before=%+v after=%+v", c.label, baselineBounds[c.label], n.Bounds)
		}
		if !regionPixelsEqual(baselineImg, inflightImg, n.Bounds) {
			t.Fatalf("unrelated control %q pixels flickered during <120ms policy switch", c.label)
		}
	}

	// Concurrent click while busy must still be blocked functionally.
	if err := tt.Click("仅接收"); err != nil {
		t.Fatal(err)
	}
	if got := d.deviceByID("peer-1").bindings[0].policy; got != wantSendOnly {
		t.Fatalf("concurrent click bypassed busy guard: policy=%+v", got)
	}

	// 2. If the write exceeds 120ms, busy visual state appears without shifting layout.
	live.busySince = time.Now().Add(-250 * time.Millisecond)
	tt.Frame()
	if !live.showBusy() {
		t.Fatal("expected showBusy() after 120ms threshold")
	}
	for _, c := range unrelatedControls {
		n, ok := tt.AccessByRole(c.role, c.label)
		if !ok || !n.Disabled {
			t.Fatalf("control %q expected disabled after >120ms slow write: ok=%v disabled=%v", c.label, ok, n.Disabled)
		}
		if n.Bounds != baselineBounds[c.label] {
			t.Fatalf("control %q shifted bounds in slow busy state: before=%+v after=%+v", c.label, baselineBounds[c.label], n.Bounds)
		}
	}

	// 3. Complete PATCH while GET /v1/status is still paused: no stale-value gap frame.
	close(patchGate)
	waitFor("PATCH completed", func() bool { return !live.busy })
	tt.Frame()
	postPatchImg := cloneRGBA(tt.Image())
	if got := d.deviceByID("peer-1").bindings[0]; got.policy != wantSendOnly || got.revision != 2 {
		t.Fatalf("post-PATCH state = (policy=%+v rev=%d), want SendOnly rev=2 before Status refresh", got.policy, got.revision)
	}
	if patchCalls.Load() != 1 {
		t.Fatalf("expected exactly 1 PATCH call (concurrent click blocked), got %d", patchCalls.Load())
	}

	// 4. Release Status refresh: settled frame matches post-PATCH frame pixel-for-pixel (no second-wave flicker).
	close(statusGate)
	waitFor("Status refresh settled", func() bool { return !live.loading })
	tt.Frame()
	settledImg := cloneRGBA(tt.Image())
	if !imagesEqual(postPatchImg, settledImg) {
		t.Fatal("Status reconciliation caused a second-wave visual flicker after PATCH already applied")
	}

	// 5. Toggle path Switch: checked flips once immediately and never bounces back during in-flight or post-PATCH.
	mu.Lock()
	patchGate = make(chan struct{})
	statusGate = make(chan struct{})
	mu.Unlock()

	if err := tt.Click("同步 Tailcat 同步"); err != nil {
		t.Fatal(err)
	}
	live.busySince = time.Now().Add(5 * time.Second)
	tt.Frame()
	swInflight, ok := tt.AccessByRole(ui.RoleSwitch, "同步 Tailcat 同步")
	if !ok || swInflight.Checked || swInflight.Disabled {
		t.Fatalf("Switch in-flight frame = (ok=%v checked=%v disabled=%v), want checked=false disabled=false", ok, swInflight.Checked, swInflight.Disabled)
	}
	close(patchGate)
	waitFor("Switch PATCH completed", func() bool { return !live.busy })
	tt.Frame()
	swPostPatch, _ := tt.AccessByRole(ui.RoleSwitch, "同步 Tailcat 同步")
	if swPostPatch.Checked || swPostPatch.Disabled || d.deviceByID("peer-1").bindings[0].revision != 3 {
		t.Fatalf("Switch post-PATCH frame bounced: checked=%v disabled=%v rev=%d", swPostPatch.Checked, swPostPatch.Disabled, d.deviceByID("peer-1").bindings[0].revision)
	}
	close(statusGate)
	waitFor("Switch Status settled", func() bool { return !live.loading })
	tt.Frame()
	swSettled, _ := tt.AccessByRole(ui.RoleSwitch, "同步 Tailcat 同步")
	if swSettled.Checked {
		t.Fatal("Switch checked unexpectedly returned to true after Status refresh")
	}

	// 6. Server error cleanly rolls back optimistic policy change.
	mu.Lock()
	patchGate = nil
	statusGate = nil
	failNextPatch = true
	// Re-enable binding on mock + local so quick preset is active.
	binding.SyncEnabled = true
	mu.Unlock()
	d.deviceByID("peer-1").bindings[0].syncEnabled = true
	tt.Frame()

	if err := tt.Click("仅接收"); err != nil {
		t.Fatal(err)
	}
	waitFor("failed PATCH rolled back", func() bool {
		return !live.busy && !live.loading && d.deviceByID("peer-1").bindings[0].policy == wantSendOnly
	})
	if got := d.deviceByID("peer-1").bindings[0].policy; got != wantSendOnly {
		t.Fatalf("failed PATCH did not roll back policy: got %+v, want %+v", got, wantSendOnly)
	}
}
