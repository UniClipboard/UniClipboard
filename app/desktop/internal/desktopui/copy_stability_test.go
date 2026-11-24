package desktopui

import (
	"bytes"
	"context"
	"encoding/json"
	"image"
	"image/color"
	"image/png"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/plugin"
)

func TestCopyMutationPreservesLayoutAndAvoidsFastBusyFlash(t *testing.T) {
	d := newDemo("history", "light")
	d.notice = "已复制"
	selected := d.current()
	if selected == nil {
		t.Fatal("expected selected demo entry")
	}
	// Ensure the selected entry has a removable custom tag so tag-row stability is covered too.
	d.tags = append(d.tags, historyTag{id: "custom-tag", name: "custom-tag", color: "blue"})
	for i := range d.entries {
		if d.entries[i].id == selected.id {
			d.entries[i].tags = append(d.entries[i].tags, "custom-tag")
			break
		}
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	live := &liveState{
		client:      api.NewClient("/nonexistent-test-sock"),
		ctx:         ctx,
		cancel:      cancel,
		jobs:        make(chan func(context.Context) func(), 4),
		results:     make(chan func(), 4),
		changed:     make(chan struct{}, 1),
		recovery:    make(chan struct{}, 1),
		items:       map[string]api.HistoryItem{selected.id: {ID: selected.id, Kind: "text"}},
		details:     map[string]entry{selected.id: *d.current()},
		detailReads: map[string]*detailRead{},
		loaded:      true,
		key:         d.liveQueryKey(),
		nextRefresh: time.Now().Add(time.Hour),
		actions: []api.Action{{
			Key:         "builtin.text/upper",
			PluginID:    "builtin.text",
			Title:       "转大写",
			Accepts:     []string{"text"},
			Placements:  []string{plugin.ActionPlacementHistoryToolbar},
			PostActions: []string{plugin.ActionPostCopy},
		}},
	}
	d.live = live
	defer live.stopWake()

	tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{})
	tt.Frame()
	baselineImg := cloneRGBA(tt.Image())

	actionsBtnBefore, ok := tt.AccessByRole(ui.RoleMenuButton, "动作")
	if !ok {
		t.Fatal("expected 动作 toolbar button before copy")
	}
	searchBefore, ok := tt.AccessByRole(ui.RoleSearch, "")
	if !ok {
		t.Fatal("expected search field before copy")
	}
	pinBtnBefore, ok := tt.AccessByRole(ui.RoleButton, "切换收藏")
	if !ok {
		t.Fatal("expected 切换收藏 toolbar button before copy")
	}
	addTagBefore, ok := tt.AccessByRole(ui.RoleButton, "添加标签")
	if !ok {
		t.Fatal("expected 添加标签 button before copy")
	}
	removeTagBefore, ok := tt.AccessByRole(ui.RoleButton, "移除 #custom-tag")
	if !ok {
		t.Fatal("expected custom tag remove button before copy")
	}

	// Start a fast copy mutation (worker has not completed yet, elapsed < 120ms).
	d.useEntry()
	if !live.busy || live.busySince.IsZero() {
		t.Fatal("expected live.busy and busySince during in-flight copy")
	}
	if live.showBusy() {
		t.Fatal("expected live.showBusy() to stay false before delay threshold")
	}
	// Pin busySince slightly ahead so slow -race software frames stay deterministically within the <120ms window.
	live.busySince = time.Now().Add(5 * time.Second)

	tt.Frame()
	fastBusyImg := cloneRGBA(tt.Image())

	// 1. No controls unmount or shift, and no "正在处理…" flash occurs while < 120ms.
	if tt.HasText("正在处理…") {
		t.Fatal("fast copy flashed 正在处理… status pill")
	}
	if !tt.HasText("已复制") {
		t.Fatal("fast copy cleared existing 已复制 status pill before delay")
	}
	actionsBtnFast, ok := tt.AccessByRole(ui.RoleMenuButton, "动作")
	if !ok {
		t.Fatal("动作 toolbar button unmounted during fast copy")
	}
	if actionsBtnFast.Bounds != actionsBtnBefore.Bounds {
		t.Fatalf("动作 button moved during fast copy: before=%+v after=%+v", actionsBtnBefore.Bounds, actionsBtnFast.Bounds)
	}
	searchFast, ok := tt.AccessByRole(ui.RoleSearch, "")
	if !ok || searchFast.Bounds != searchBefore.Bounds {
		t.Fatalf("search field moved during fast copy: ok=%v before=%+v after=%+v", ok, searchBefore.Bounds, searchFast.Bounds)
	}
	pinBtnFast, ok := tt.AccessByRole(ui.RoleButton, "切换收藏")
	if !ok || pinBtnFast.Bounds != pinBtnBefore.Bounds {
		t.Fatalf("切换收藏 button moved during fast copy: ok=%v before=%+v after=%+v", ok, pinBtnBefore.Bounds, pinBtnFast.Bounds)
	}
	addTagFast, ok := tt.AccessByRole(ui.RoleButton, "添加标签")
	if !ok || addTagFast.Bounds != addTagBefore.Bounds {
		t.Fatalf("添加标签 button moved during fast copy: ok=%v before=%+v after=%+v", ok, addTagBefore.Bounds, addTagFast.Bounds)
	}
	removeTagFast, ok := tt.AccessByRole(ui.RoleButton, "移除 #custom-tag")
	if !ok || removeTagFast.Bounds != removeTagBefore.Bounds {
		t.Fatalf("custom tag remove button changed during fast copy: ok=%v before=%+v after=%+v", ok, removeTagBefore.Bounds, removeTagFast.Bounds)
	}
	if !imagesEqual(baselineImg, fastBusyImg) {
		t.Fatal("fast in-flight copy produced visual flicker compared to idle frame")
	}

	// 2. Functional guard still blocks concurrent mutations while live.busy is true.
	if len(live.jobs) != 1 {
		t.Fatalf("expected 1 queued job, got %d", len(live.jobs))
	}
	d.useEntry()
	d.togglePin()
	d.openTagEditor()
	if len(live.jobs) != 1 || d.tagEditor.open {
		t.Fatalf("busy guard failed: jobs=%d tagEditorOpen=%v", len(live.jobs), d.tagEditor.open)
	}

	// 3. Once busy exceeds the delay threshold (> 120ms), busy cues appear WITHOUT shifting layout.
	live.busySince = time.Now().Add(-250 * time.Millisecond)
	tt.Frame()
	if !live.showBusy() {
		t.Fatal("expected live.showBusy() after delay threshold")
	}
	if !tt.HasText("正在处理…") {
		t.Fatal("expected 正在处理… status pill once mutation exceeds delay threshold")
	}
	actionsBtnSlow, ok := tt.AccessByRole(ui.RoleMenuButton, "动作")
	if !ok {
		t.Fatal("动作 toolbar button unmounted during slow mutation")
	}
	if actionsBtnSlow.Bounds != actionsBtnBefore.Bounds {
		t.Fatalf("动作 button shifted during slow mutation: before=%+v after=%+v", actionsBtnBefore.Bounds, actionsBtnSlow.Bounds)
	}
	searchSlow, ok := tt.AccessByRole(ui.RoleSearch, "")
	if !ok || searchSlow.Bounds != searchBefore.Bounds {
		t.Fatalf("search field shifted during slow mutation: ok=%v before=%+v after=%+v", ok, searchBefore.Bounds, searchSlow.Bounds)
	}
	if !actionsBtnSlow.Disabled {
		t.Fatal("expected 动作 button to be disabled during slow mutation")
	}
	pinBtnSlow, ok := tt.AccessByRole(ui.RoleButton, "切换收藏")
	if !ok || pinBtnSlow.Bounds != pinBtnBefore.Bounds || !pinBtnSlow.Disabled {
		t.Fatalf("切换收藏 button shifted or not disabled during slow mutation: ok=%v bounds=%+v disabled=%v", ok, pinBtnSlow.Bounds, pinBtnSlow.Disabled)
	}
	addTagSlow, ok := tt.AccessByRole(ui.RoleButton, "添加标签")
	if !ok || addTagSlow.Bounds != addTagBefore.Bounds || !addTagSlow.Disabled {
		t.Fatalf("添加标签 button shifted or not disabled during slow mutation: ok=%v bounds=%+v disabled=%v", ok, addTagSlow.Bounds, addTagSlow.Disabled)
	}
	removeTagSlow, ok := tt.AccessByRole(ui.RoleButton, "移除 #custom-tag")
	if !ok || removeTagSlow.Bounds != removeTagBefore.Bounds {
		t.Fatalf("custom tag remove button shifted during slow mutation: ok=%v before=%+v after=%+v", ok, removeTagBefore.Bounds, removeTagSlow.Bounds)
	}

	// 4. Completing the mutation restores idle state cleanly.
	live.stopBusy()
	d.notice = "已复制"
	tt.Frame()
	if tt.HasText("正在处理…") || !tt.HasText("已复制") {
		t.Fatal("expected status pill to return to 已复制 after completion")
	}
}

func TestFavoriteAndTagMutationPreservesLayoutAndDetailCache(t *testing.T) {
	dir, err := os.MkdirTemp("/tmp", "unic-fav-stable-")
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

	var pngBuf bytes.Buffer
	img := image.NewRGBA(image.Rect(0, 0, 4, 3))
	img.SetRGBA(0, 0, color.RGBA{R: 43, G: 127, B: 255, A: 255})
	if err := png.Encode(&pngBuf, img); err != nil {
		t.Fatal(err)
	}

	var mu sync.Mutex
	historyRev := uint64(1)
	items := map[string]*api.HistoryItem{
		"text-1": {
			ID:         "text-1",
			Kind:       "text",
			Origin:     "local_system",
			Revision:   1,
			CreatedAt:  time.Now().UnixMilli(),
			Tags:       []string{"custom-tag"},
			TotalBytes: 42,
			Parts:      []api.HistoryPart{{Available: true, Kind: "representation", MIME: "text/plain", Size: 42}},
		},
		"img-1": {
			ID:         "img-1",
			Kind:       "image",
			Origin:     "local_system",
			Revision:   1,
			CreatedAt:  time.Now().UnixMilli() - 1000,
			Tags:       []string{},
			TotalBytes: int64(pngBuf.Len()),
			Parts:      []api.HistoryPart{{Available: true, Kind: "representation", MIME: "image/png", Size: int64(pngBuf.Len())}},
		},
	}
	var textContentFetches, imgContentFetches atomic.Int32

	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.Status{ID: "svc-fav", Name: "TestMac"})
	})
	mux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(config.State{Revision: "rev-1", Config: config.Default()})
	})
	mux.HandleFunc("GET /v1/actions", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"actions": []api.Action{{
			Key:         "builtin.text/upper",
			PluginID:    "builtin.text",
			Title:       "转大写",
			Accepts:     []string{"text"},
			Placements:  []string{plugin.ActionPlacementHistoryToolbar},
			PostActions: []string{plugin.ActionPostCopy},
		}}})
	})
	mux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.PluginInfo{}) })
	mux.HandleFunc("GET /v1/sync-backends", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.SyncBackendInfo{}) })
	mux.HandleFunc("GET /v1/history/tags", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode([]string{"custom-tag", "extra-tag"})
	})
	mux.HandleFunc("GET /v1/candidates", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.Candidate{}) })
	mux.HandleFunc("GET /v1/events", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		<-r.Context().Done()
	})
	mux.HandleFunc("POST /v1/history/search/counts", func(w http.ResponseWriter, r *http.Request) {
		var req engine.HistorySearchCountsRequest
		_ = json.NewDecoder(r.Body).Decode(&req)
		counts := make([]uint64, len(req.Queries))
		_ = json.NewEncoder(w).Encode(map[string]any{"counts": counts})
	})
	mux.HandleFunc("POST /v1/history/search/candidates", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"candidates": []any{}})
	})
	mux.HandleFunc("GET /v1/history", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		rev := historyRev
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(api.HistoryPage{Items: []api.HistoryItem{}, Revision: rev})
	})
	mux.HandleFunc("POST /v1/history/search", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		rev := historyRev
		t1 := *items["text-1"]
		t1.Tags = slices.Clone(t1.Tags)
		t1.Parts = slices.Clone(t1.Parts)
		i1 := *items["img-1"]
		i1.Tags = slices.Clone(i1.Tags)
		i1.Parts = slices.Clone(i1.Parts)
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(api.HistorySearchPage{
			Hits: []api.HistorySearchHit{
				{Item: t1, Preview: engine.HistorySearchPreview{Text: "https://github.com/UniClipboard/UniClipboard"}},
				{Item: i1, Preview: engine.HistorySearchPreview{Text: "图片"}},
			},
			HistoryRevision: rev,
		})
	})
	mux.HandleFunc("GET /v1/history/{id}", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		it := *items[r.PathValue("id")]
		it.Tags = slices.Clone(it.Tags)
		it.Parts = slices.Clone(it.Parts)
		mu.Unlock()
		_ = json.NewEncoder(w).Encode(it)
	})
	mux.HandleFunc("GET /v1/history/{id}/content", func(w http.ResponseWriter, r *http.Request) {
		if r.PathValue("id") == "img-1" {
			imgContentFetches.Add(1)
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write(pngBuf.Bytes())
			return
		}
		textContentFetches.Add(1)
		w.Header().Set("Content-Type", "text/plain; charset=utf-8")
		_, _ = w.Write([]byte("https://github.com/UniClipboard/UniClipboard/releases"))
	})
	mux.HandleFunc("PATCH /v1/history/{id}", func(w http.ResponseWriter, r *http.Request) {
		var u api.HistoryUpdate
		_ = json.NewDecoder(r.Body).Decode(&u)
		mu.Lock()
		it := items[r.PathValue("id")]
		if u.Tags != nil {
			it.Tags = slices.Clone(*u.Tags)
		}
		it.Revision++
		historyRev++
		out := *it
		out.Tags = slices.Clone(out.Tags)
		out.Parts = slices.Clone(out.Parts)
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

	tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{})
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
		t.Fatalf("timed out waiting for %s (page=%q loaded=%v loading=%v detailBusy=%v readErr=%q msg=%q selected=%q entries=%d actions=%d cur=%+v)",
			desc, d.page, live.loaded, live.loading, live.detailBusy, live.readError, live.message, d.selected, len(d.entries), len(live.actions), d.current())
	}

	// Wait for initial load and detail hydration of text-1.
	waitFor("initial text-1 detail", func() bool {
		return live.loaded && !live.loading && !live.detailBusy &&
			d.current() != nil && d.current().id == "text-1" &&
			d.current().text == "https://github.com/UniClipboard/UniClipboard/releases" &&
			len(live.actions) == 1
	})

	actionsBefore, ok := tt.AccessByRole(ui.RoleMenuButton, "动作")
	if !ok {
		t.Fatal("expected 动作 button before favorite")
	}
	searchBefore, ok := tt.AccessByRole(ui.RoleSearch, "")
	if !ok {
		t.Fatal("expected search field before favorite")
	}
	pinBefore, ok := tt.AccessByRole(ui.RoleButton, "切换收藏")
	if !ok {
		t.Fatal("expected 切换收藏 button before favorite")
	}
	addTagBefore, ok := tt.AccessByRole(ui.RoleButton, "添加标签")
	if !ok {
		t.Fatal("expected 添加标签 button before favorite")
	}
	removeTagBefore, ok := tt.AccessByRole(ui.RoleButton, "移除 #custom-tag")
	if !ok {
		t.Fatal("expected custom tag remove button before favorite")
	}
	initialTextFetches := textContentFetches.Load()
	initialSerial := live.contentSerial

	// Toggle favorite ON and observe every frame until reconciliation completes.
	d.togglePin()
	assertStable := func(stage string) {
		t.Helper()
		if live.detailBusy {
			t.Fatalf("%s: detailBusy flashed true during metadata-only revision bump", stage)
		}
		if d.current() == nil || d.current().text != "https://github.com/UniClipboard/UniClipboard/releases" {
			t.Fatalf("%s: text detail was wiped during favorite: %+v", stage, d.current())
		}
		if a, ok := tt.AccessByRole(ui.RoleMenuButton, "动作"); !ok || a.Bounds != actionsBefore.Bounds {
			t.Fatalf("%s: 动作 button shifted or unmounted: ok=%v before=%+v got=%+v", stage, ok, actionsBefore.Bounds, a.Bounds)
		}
		if s, ok := tt.AccessByRole(ui.RoleSearch, ""); !ok || s.Bounds != searchBefore.Bounds {
			t.Fatalf("%s: search field shifted: ok=%v before=%+v got=%+v", stage, ok, searchBefore.Bounds, s.Bounds)
		}
		if p, ok := tt.AccessByRole(ui.RoleButton, "切换收藏"); !ok || p.Bounds != pinBefore.Bounds {
			t.Fatalf("%s: 切换收藏 button shifted: ok=%v before=%+v got=%+v", stage, ok, pinBefore.Bounds, p.Bounds)
		}
		if r, ok := tt.AccessByRole(ui.RoleButton, "移除 #custom-tag"); !ok || r.Bounds != removeTagBefore.Bounds {
			t.Fatalf("%s: custom tag remove button shifted: ok=%v before=%+v got=%+v", stage, ok, removeTagBefore.Bounds, r.Bounds)
		}
		if add, ok := tt.AccessByRole(ui.RoleButton, "添加标签"); !ok || add.Bounds != addTagBefore.Bounds {
			t.Fatalf("%s: 添加标签 shifted: ok=%v before=%+v got=%+v", stage, ok, addTagBefore.Bounds, add.Bounds)
		}
	}

	waitFor("favorite applied and reconciled", func() bool {
		assertStable("favorite-on")
		return !live.busy && !live.loading && d.current() != nil && d.current().pinned && live.items["text-1"].Revision == 2
	})
	if textContentFetches.Load() != initialTextFetches {
		t.Fatalf("favorite triggered redundant content reload: before=%d after=%d", initialTextFetches, textContentFetches.Load())
	}
	if live.contentSerial != initialSerial {
		t.Fatalf("favorite dropped detail cache (contentSerial %d -> %d)", initialSerial, live.contentSerial)
	}

	// Add another custom tag — must also preserve detail cache without dropping preview.
	d.setEntryTags("text-1", []string{"custom-tag", "extra-tag"})
	waitFor("tag edit reconciled", func() bool {
		if live.detailBusy {
			t.Fatal("tag edit flashed detailBusy")
		}
		return !live.busy && !live.loading && live.items["text-1"].Revision == 3
	})
	if textContentFetches.Load() != initialTextFetches {
		t.Fatalf("tag edit triggered redundant content reload: before=%d after=%d", initialTextFetches, textContentFetches.Load())
	}

	// Now select the image entry and favorite it: image preview and quick-look button must stay intact with 0 re-fetches!
	d.selectEntry("img-1")
	waitFor("img-1 detail loaded", func() bool {
		return !live.detailBusy && d.current() != nil && d.current().id == "img-1" && d.current().image != nil
	})
	imgFetchesBefore := imgContentFetches.Load()
	imgSerialBefore := live.contentSerial

	d.togglePin()
	waitFor("img-1 favorite reconciled", func() bool {
		if live.detailBusy {
			t.Fatal("favoriting image flashed detailBusy")
		}
		if d.current() == nil || d.current().image == nil {
			t.Fatal("favoriting image cleared image preview")
		}
		if _, ok := tt.AccessByRole(ui.RoleButton, "快速查看"); !ok {
			t.Fatal("favoriting image hid 快速查看 button")
		}
		return !live.busy && !live.loading && d.current().pinned && live.items["img-1"].Revision == 2
	})
	if imgContentFetches.Load() != imgFetchesBefore {
		t.Fatalf("favoriting image re-fetched PNG: before=%d after=%d", imgFetchesBefore, imgContentFetches.Load())
	}
	if live.contentSerial != imgSerialBefore {
		t.Fatalf("favoriting image dropped cached image (contentSerial %d -> %d)", imgSerialBefore, live.contentSerial)
	}

	// Sanity check: an actual Parts availability change MUST still invalidate cached detail.
	oldItem := api.HistoryItem{Kind: "image", TotalBytes: 100, Parts: []api.HistoryPart{{Available: false, Kind: "representation", MIME: "image/png", Size: 100}}}
	newItem := api.HistoryItem{Kind: "image", TotalBytes: 100, Parts: []api.HistoryPart{{Available: true, Kind: "representation", MIME: "image/png", Size: 100}}}
	if !historyContentChanged(oldItem, newItem) {
		t.Fatal("expected Parts.Available change to count as content change")
	}
	sameContent := newItem
	sameContent.Revision++
	sameContent.Tags = []string{plugin.TagFavorite}
	if historyContentChanged(newItem, sameContent) {
		t.Fatal("expected metadata-only favorite change not to count as content change")
	}
}

func cloneRGBA(src *image.RGBA) *image.RGBA {
	dst := image.NewRGBA(src.Bounds())
	copy(dst.Pix, src.Pix)
	return dst
}

func imagesEqual(a, b image.Image) bool {
	if a.Bounds() != b.Bounds() {
		return false
	}
	bounds := a.Bounds()
	for y := bounds.Min.Y; y < bounds.Max.Y; y++ {
		for x := bounds.Min.X; x < bounds.Max.X; x++ {
			if a.At(x, y) != b.At(x, y) {
				return false
			}
		}
	}
	return true
}
