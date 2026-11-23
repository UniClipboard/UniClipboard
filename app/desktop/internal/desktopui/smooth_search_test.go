package desktopui

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func testBitmap(w, h int) *ui.Bitmap {
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			img.SetRGBA(x, y, color.RGBA{R: 43, G: 127, B: 255, A: 255})
		}
	}
	return ui.NewBitmap(img)
}

func TestHistoryHighlightTermsCoverage(t *testing.T) {
	d := newDemo("history", "light")
	d.live = &liveState{}
	// Span beyond the 90-rune title truncation must still be extracted from preview.
	longPreview := strings.Repeat("a", 100) + "TargetKeyword tail"
	v := entry{
		title:      shortText(longPreview, 90),
		preview:    longPreview,
		highlights: []engine.HistorySearchRange{{Start: 100, End: 113}},
	}
	terms := d.historyHighlightTerms(v)
	if len(terms) != 1 || terms[0] != "TargetKeyword" {
		t.Fatalf("expected TargetKeyword from preview span, got %v", terms)
	}

	// Fallback to free-text query words while skipping filter tokens.
	d.query = "#work @mac /code Release alpha"
	fallback := d.historyHighlightTerms(entry{title: "Release notes"})
	if len(fallback) != 2 || fallback[0] != "Release" || fallback[1] != "alpha" {
		t.Fatalf("expected free-text fallback [Release alpha], got %v", fallback)
	}
}

func newStaticLiveState() *liveState {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	return &liveState{
		client:      api.NewClient("/nonexistent-test-sock"),
		ctx:         ctx,
		cancel:      cancel,
		results:     make(chan func(), 4),
		changed:     make(chan struct{}, 1),
		recovery:    make(chan struct{}, 1),
		items:       map[string]api.HistoryItem{},
		details:     map[string]entry{},
		detailReads: map[string]*detailRead{},
		loading:     true, // keep poll() from launching a nil-client loadHistory
		nextRefresh: time.Now().Add(time.Hour),
	}
}

func TestSmoothSearchSubtitleAndColdStartSkeleton(t *testing.T) {
	t.Run("stale-subtitle-preserved-during-pending-query", func(t *testing.T) {
		d := newDemo("history", "light")
		l := newStaticLiveState()
		l.loaded = true
		l.foregroundLoading = true
		l.loadingSince = time.Now()
		l.key = d.liveQueryKey()
		l.displayedKey = "old-key"
		d.live = l
		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{})
		wantCount := d.historyCountLabel()
		// Fast (<120ms) query keeps previous count visible without blanking to "".
		foundCount := false
		for _, txt := range tt.Texts() {
			if txt == wantCount {
				foundCount = true
				break
			}
		}
		if !foundCount {
			t.Fatalf("expected stale count %q preserved during fast pending query, texts=%v", wantCount, tt.Texts())
		}

		// Slow (>120ms) query still keeps previous results & subtitle while showing loading cues.
		d.live.loadingSince = time.Now().Add(-300 * time.Millisecond)
		tt.Frame()
		_ = tt.Image()
		if !d.live.showForegroundLoading() {
			t.Fatal("expected showForegroundLoading after 120ms threshold")
		}
	})

	t.Run("cold-start-skeleton-keeps-split-geometry", func(t *testing.T) {
		for _, layout := range []string{"list", "grid"} {
			d := newDemo("history", "light")
			d.layout = layout
			d.entries = nil
			d.selected = ""
			l := newStaticLiveState()
			l.loaded = false
			l.foregroundLoading = true
			l.loadingSince = time.Now().Add(-250 * time.Millisecond)
			l.key = d.liveQueryKey()
			d.live = l
			tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{})
			_ = tt.Image()
			if tt.HasText("还没有剪贴板历史") || tt.HasText("正在读取历史…") {
				t.Fatalf("%s: cold start >120ms should render structural skeleton, not empty state or lone spinner text: %v", layout, tt.Texts())
			}
		}
	})
}

func TestProgressiveDetailAndHighlightViews(t *testing.T) {
	for _, theme := range []string{"light", "dark"} {
		d := newDemo("history", theme)
		d.query = "release"
		l := newStaticLiveState()
		l.loaded = true
		l.detailBusy = true
		l.detailLoadingSince = time.Now().Add(-250 * time.Millisecond)
		l.key = d.liveQueryKey()
		l.displayedKey = l.key
		d.live = l

		// 1. Long text detail loading shows preview with highlights + streaming tail shimmer.
		longText := "Desktop Alpha release checklist: " + strings.Repeat("verify smooth search transition ", 4)
		d.entries = []entry{
			{
				id: "txt-long", kind: desktopkit.KindText, title: "Desktop Alpha release checklist",
				preview: longText, date: "今天 18:20", source: "local",
				highlights: []engine.HistorySearchRange{{Start: 14, End: 21}},
			},
			{
				id: "img-up", kind: desktopkit.KindImage, title: "release_spec.png",
				preview: "release_spec.png", date: "今天 18:10", source: "local",
				image:   testBitmap(64, 40),
				picture: &historyImage{edge: 256},
			},
			{
				id: "img-nil", kind: desktopkit.KindImage, title: "pending_thumb.png",
				preview: "pending_thumb.png", date: "今天 18:05", source: "local",
			},
			{
				id: "folder-1", kind: desktopkit.KindFolder, title: "release-artifacts/",
				preview: "release-artifacts/", date: "今天 18:00", source: "local",
			},
		}
		d.selectEntry("txt-long")
		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{Dark: theme == "dark"})
		if !tt.HasText(longText) {
			t.Fatalf("%s: expected loading preview text visible, got %v", theme, tt.Texts())
		}
		_ = tt.Image()

		// 2. Image upgrading from cached thumbnail (edge > 0 -> 0) shows blur-up + HD badge after 120ms.
		d.live.detailBusy = false
		d.selectEntry("img-up")
		tt.Frame()
		if !tt.HasText("正在加载原图…") {
			t.Fatalf("%s: expected full-image upgrade badge on cached thumbnail, got %v", theme, tt.Texts())
		}
		_ = tt.Image()

		// 3. Uncached image (img == nil) renders shimmer placeholder without panic.
		d.live.detailBusy = true
		d.selectEntry("img-nil")
		tt.Frame()
		_ = tt.Image()

		// 4. Folder detail loading renders structured skeleton rather than plain loading text.
		d.selectEntry("folder-1")
		tt.Frame()
		if tt.HasText("正在读取内容…") {
			t.Fatalf("%s: folder loading should render structured skeleton instead of plain loading text", theme)
		}
		_ = tt.Image()

		// 5. Grid view + Grid Detail Drawer with search highlights & shimmer image card.
		d.layout = "grid"
		d.inspector = true
		d.selectEntry("txt-long")
		tt.Frame()
		_ = tt.Image()
	}
}

func TestQuickPanelDebouncePreservesRows(t *testing.T) {
	d := newDemo("history", "light")
	d.live = newStaticLiveState()
	d.live.loaded = true
	p := &quickPanelState{
		theme:    "light",
		mode:     "history",
		visible:  true,
		hovered:  -1,
		keyboard: true,
		live:     &quickLive{d: d},
		rows: []qpEntry{
			{id: "q1", title: "docker compose up --build", kind: "代码", source: "本机", age: "18:20"},
			{id: "q2", title: "Desktop Alpha release notes", kind: "文本", source: "本机", age: "18:10"},
		},
	}
	p.now = time.Now()
	p.query = "docker"
	p.queryDue = p.now.Add(300 * time.Millisecond)

	if len(p.rows) == 0 {
		t.Fatal("fixture quick panel rows unexpectedly empty")
	}
	firstTitle := p.rows[0].title

	tt := ui.NewTesterWith(p.panel, 600, 440, ui.TesterOptions{})
	if tt.HasText("正在搜索…") {
		t.Fatalf("quick panel must not wipe existing rows with 正在搜索… during 300ms debounce: %v", tt.Texts())
	}
	if !tt.HasText(firstTitle) {
		t.Fatalf("expected existing quick panel row %q preserved during debounce, got %v", firstTitle, tt.Texts())
	}

	// Only when rows are empty during pending debounce/load should 正在搜索… appear.
	p.rows = nil
	tt.Frame()
	if !tt.HasText("正在搜索…") {
		t.Fatalf("expected 正在搜索… when quick panel has zero rows during pending search, got %v", tt.Texts())
	}

	// Verify qpMarkedSpans uses live query while staleQuery is true.
	entryWithOldSpans := qpEntry{
		title:      "docker compose up",
		highlights: []engine.HistorySearchRange{{Start: 7, End: 14}}, // old span for "compose"
	}
	spans := qpMarkedSpans(entryWithOldSpans, "docker", false, true, ui.RGB(20, 20, 20), ui.RGB(100, 100, 100))
	if len(spans) != 2 || spans[0].Text != "docker" || spans[0].Background == (ui.Color{}) || spans[1].Text != " compose up" || spans[1].Background != (ui.Color{}) {
		t.Fatalf("expected coalesced live query 'docker' highlight instead of stale 'compose' span, got %+v", spans)
	}
}

func TestSelectionContinuityVsScopeSwitch(t *testing.T) {
	dir, err := os.MkdirTemp("/tmp", "unic-smooth-")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(dir)
	sock := filepath.Join(dir, "s.sock")
	ln, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()

	makeHit := func(id, text string, pinned bool) api.HistorySearchHit {
		tags := []string{}
		if pinned {
			tags = []string{"favorite"}
		}
		return api.HistorySearchHit{
			Item: api.HistoryItem{
				ID:        id,
				Kind:      "text",
				Revision:  1,
				CreatedAt: time.Now().UnixMilli(),
				Tags:      tags,
			},
			Preview: engine.HistorySearchPreview{Text: text},
		}
	}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(api.Status{ID: "svc-smooth", Name: "TestMac"})
	})
	mux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(config.State{Revision: "rev-1", Config: config.Default()})
	})
	mux.HandleFunc("GET /v1/actions", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.Action{}) })
	mux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.PluginInfo{}) })
	mux.HandleFunc("GET /v1/sync-backends", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.SyncBackendInfo{}) })
	mux.HandleFunc("GET /v1/history/tags", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]string{}) })
	mux.HandleFunc("GET /v1/candidates", func(w http.ResponseWriter, r *http.Request) { _ = json.NewEncoder(w).Encode([]api.Candidate{}) })
	mux.HandleFunc("GET /v1/history/{id}/content", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"parts": []map[string]any{{"kind": "text", "mime": "text/plain", "text": "body"}}})
	})
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
		_ = json.NewEncoder(w).Encode(api.HistoryPage{Items: []api.HistoryItem{}, Revision: 1})
	})
	mux.HandleFunc("POST /v1/history/search", func(w http.ResponseWriter, r *http.Request) {
		var req engine.HistorySearchRequest
		_ = json.NewDecoder(r.Body).Decode(&req)
		q := req.Query
		var hits []api.HistorySearchHit
		if len(q.Tags) > 0 && q.Tags[0] == "favorite" {
			hits = []api.HistorySearchHit{makeHit("h3", "Favorite release", true), makeHit("h1", "Alpha release", true)}
		} else if len(q.TextTerms) > 0 && q.TextTerms[0] == "only-h1" {
			hits = []api.HistorySearchHit{makeHit("h1", "only-h1 release", true)}
		} else if len(q.TextTerms) > 0 && q.TextTerms[0] == "release" {
			hits = []api.HistorySearchHit{makeHit("h1", "Alpha release", true), makeHit("h2", "Beta release", false)}
		} else {
			hits = []api.HistorySearchHit{makeHit("h1", "Alpha release", true), makeHit("h2", "Beta release", false), makeHit("h3", "Favorite release", true)}
		}
		_ = json.NewEncoder(w).Encode(api.HistorySearchPage{Hits: hits, HistoryRevision: 1})
	})

	srv := &http.Server{Handler: mux}
	go func() { _ = srv.Serve(ln) }()
	defer srv.Close()

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	desktop := NewDesktop(ctx, api.NewClient(sock))
	defer desktop.Close()
	d := desktop.d

	spin := func(label string, cond func() bool) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			d.live.poll(d)
			if cond() {
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		t.Fatalf("timeout: %s (loaded=%v loading=%v selected=%q kind=%q entries=%d)",
			label, d.live.loaded, d.live.loading, d.selected, d.live.lastResultKind, len(d.entries))
	}

	// 1. Cold start loads initial items and selects h1.
	spin("cold load", func() bool { return d.live.loaded && !d.live.loading && len(d.entries) == 3 })
	if d.selected != "h1" || d.live.lastResultKind != "cold" {
		t.Fatalf("cold load: selected=%q kind=%q, want h1/cold", d.selected, d.live.lastResultKind)
	}

	// 2. User selects h2 and refines query in same scope; h2 remains in results -> stays selected!
	d.selectEntry("h2")
	d.query = "release"
	spin("refine keep selection", func() bool { return !d.live.loading && d.live.displayedKey == d.liveQueryKey() })
	if d.selected != "h2" {
		t.Fatalf("expected selection continuity to keep h2, got %q", d.selected)
	}
	if d.live.lastResultKind != "refine" {
		t.Fatalf("expected lastResultKind=refine, got %q", d.live.lastResultKind)
	}

	// 3. Refine query so h2 is filtered out -> falls back to first result h1.
	d.query = "only-h1"
	spin("refine fallback", func() bool { return !d.live.loading && d.live.displayedKey == d.liveQueryKey() })
	if d.selected != "h1" || d.live.lastResultKind != "refine" {
		t.Fatalf("expected fallback to h1/refine, got %q/%q", d.selected, d.live.lastResultKind)
	}

	// 4. Switch sidebar scope (all -> pinned): even though h1 is still in hits, scope switch selects first item h3.
	d.query = ""
	d.nav.Value = "pinned"
	spin("scope switch", func() bool { return !d.live.loading && d.live.displayedKey == d.liveQueryKey() })
	if d.selected != "h3" || d.live.lastResultKind != "scope" {
		t.Fatalf("expected scope switch to select h3/scope, got %q/%q", d.selected, d.live.lastResultKind)
	}
}

func makeShowcaseBitmap(w, h int, accent color.RGBA) *ui.Bitmap {
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		ty := float64(y) / float64(max(1, h-1))
		for x := 0; x < w; x++ {
			tx := float64(x) / float64(max(1, w-1))
			r := uint8(float64(accent.R)*(1-0.45*tx) + 20*ty)
			g := uint8(float64(accent.G)*(1-0.35*ty) + 60*tx)
			b := uint8(min(255, int(float64(accent.B)*(0.75+0.25*ty)+35*tx)))
			// Crisp geometric card grid inside the graphic so blur-up vs sharp is unmistakable.
			if x >= w/10 && x <= w*9/10 && y >= h/8 && y <= h*7/8 {
				r = uint8(min(255, int(r)/2+16))
				g = uint8(min(255, int(g)/2+20))
				b = uint8(min(255, int(b)/2+30))
				if (x-w/10 < 3) || (w*9/10-x < 3) || (y-h/8 < 3) || (h*7/8-y < 3) {
					r, g, b = 220, 234, 255
				}
				// High-contrast mock UI bars inside card.
				if y >= h*2/8 && y <= h*2/8+h/14 && x >= w*2/10 && x <= w*5/10 {
					r, g, b = 248, 250, 255
				}
				if y >= h*3/8 && y <= h*3/8+h/22 && x >= w*2/10 && x <= w*7/10 {
					r, g, b = 165, 200, 255
				}
				if y >= h*5/10 && y <= h*78/100 && x >= w*2/10 && x <= w*46/100 {
					r, g, b = 74, 152, 255
					if (x/12+y/12)%2 == 0 {
						r, g, b = 122, 184, 255
					}
				}
				if y >= h*5/10 && y <= h*62/100 && x >= w*50/100 && x <= w*82/100 {
					r, g, b = 56, 205, 165
				}
				if y >= h*66/100 && y <= h*78/100 && x >= w*50/100 && x <= w*82/100 {
					r, g, b = 250, 176, 68
				}
			}
			img.SetRGBA(x, y, color.RGBA{R: r, G: g, B: b, A: 255})
		}
	}
	var buf bytes.Buffer
	_ = png.Encode(&buf, img)
	if bm, err := ui.DecodeBitmap(buf.Bytes()); err == nil {
		return bm
	}
	return ui.NewBitmap(img)
}

func savePNGFile(t *testing.T, path string, img image.Image) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := png.Encode(f, img); err != nil {
		_ = f.Close()
		t.Fatal(err)
	}
	if err := f.Close(); err != nil {
		t.Fatal(err)
	}
}

func makeEvidenceEntries(fullImg, thumbImg *ui.Bitmap) []entry {
	longText := "Desktop Alpha build 4 release checklist: verify BoardUI search smoothness, stale subtitle preservation, and Quick Panel debounce latency."
	longFull := longText + "\n\n1. Confirm 2px top hairline progress bar glides smoothly during >120ms queries.\n2. Verify search highlights cover List Title, Excerpt, Grid Cards, Detail & Quick Panel.\n3. Confirm selection continuity keeps the active item selected across same-scope refinements."
	return []entry{
		{
			id: "e1", kind: desktopkit.KindText, title: "Desktop Alpha build 4 release checklist",
			preview: longText, text: longFull, date: "今天 18:24", source: "local",
			sourceAppName: "Cursor", pinned: true, tags: []string{"work"}, size: "268 字符",
			highlights: []engine.HistorySearchRange{{Start: 22, End: 29}},
		},
		{
			id: "e2", kind: desktopkit.KindCode, title: "docker compose up -d --build && go test -race ./app/desktop/...",
			preview: "docker compose up -d --build && go test -race ./app/desktop/... -run Release",
			text:    "# Verify release build & desktop smoothness tests\ndocker compose up -d --build\ngo test -race ./app/desktop/internal/desktopui -run TestSelectionContinuity",
			date: "今天 18:10", source: "local", sourceAppName: "WezTerm", tags: []string{"work"}, size: "152 字符",
			highlights: []engine.HistorySearchRange{{Start: 69, End: 76}},
		},
		{
			id: "e3", kind: desktopkit.KindImage, title: "BoardUI_Release_Spec.png",
			preview: "BoardUI_Release_Spec.png", date: "今天 17:52", source: "local",
			sourceAppName: "Figma", pinned: true, tags: []string{"design"}, size: "1600 × 1000 · 286 KB",
			image: fullImg, picture: &historyImage{bitmap: fullImg, edge: 0, width: 1600, height: 1000, released: true},
		},
		{
			id: "e4", kind: desktopkit.KindLink, title: "UniClipboard · History Search Contract Release Spec",
			preview: "https://github.com/UniClipboard/uniclipboard/blob/main/docs/contracts/history-search-contract.md",
			text:    "https://github.com/UniClipboard/uniclipboard/blob/main/docs/contracts/history-search-contract.md",
			date: "今天 17:15", source: "local", sourceAppName: "Safari", size: "96 字符",
		},
		{
			id: "e5", kind: desktopkit.KindImage, title: "QuickPanel_Release_Capture.png",
			preview: "QuickPanel_Release_Capture.png", date: "今天 16:40", source: "local",
			sourceAppName: "CleanShot X", size: "2560 × 1440 · 512 KB",
			// Intentionally uncached image thumbnail for shimmer proof.
			image: nil,
		},
		{
			id: "e6", kind: desktopkit.KindFolder, title: "release-alpha-4-artifacts/",
			preview: "release-alpha-4-artifacts/", date: "昨天 22:05", source: "local",
			sourceAppName: "Finder", size: "14 个文件 · 8.4 MB",
		},
	}
}

// TestCaptureSmoothSearchEvidence renders real mygo HiDPI screenshots and animation frame
// sequences when UNIC_SMOOTH_SEARCH_EVIDENCE is set to an output directory.
func TestCaptureSmoothSearchEvidence(t *testing.T) {
	dir := os.Getenv("UNIC_SMOOTH_SEARCH_EVIDENCE")
	if dir == "" {
		t.Skip("set UNIC_SMOOTH_SEARCH_EVIDENCE to generate visual evidence")
	}

	fullImg := makeShowcaseBitmap(640, 400, color.RGBA{R: 37, G: 99, B: 235, A: 255})
	thumbImg := makeShowcaseBitmap(128, 80, color.RGBA{R: 37, G: 99, B: 235, A: 255})

	// ---------------- 1. Static 2x HiDPI Screenshots ----------------
	// Shot 01: List + Detail search highlights (Light)
	{
		d := newDemo("history", "light")
		l := newStaticLiveState()
		l.loaded = true
		d.query = "release"
		l.key = d.liveQueryKey()
		l.displayedKey = l.key
		l.displayedScope = d.liveScopeKey()
		d.live = l
		d.entries = makeEvidenceEntries(fullImg, thumbImg)
		d.selectEntry("e1")
		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{})
		tt.SetScale(2)
		tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
		tt.Advance(240 * time.Millisecond)
		savePNGFile(t, filepath.Join(dir, "01-search-highlights-list-light.png"), tt.Image())
	}

	// Shot 02: Grid + 436px Floating Detail Drawer + Shimmer uncached image card + Highlights (Dark)
	{
		d := newDemo("history", "dark")
		d.layout = "grid"
		d.inspector = true
		d.query = "release"
		l := newStaticLiveState()
		l.loaded = true
		l.key = d.liveQueryKey()
		l.displayedKey = l.key
		l.displayedScope = d.liveScopeKey()
		d.live = l
		d.entries = makeEvidenceEntries(fullImg, thumbImg)
		d.selectEntry("e2")
		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{Dark: true})
		tt.SetScale(2)
		tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
		tt.Advance(320 * time.Millisecond)
		savePNGFile(t, filepath.Join(dir, "02-search-highlights-grid-drawer-dark.png"), tt.Image())
	}

	// Shot 03: Slow Query (>120ms) — 2px Top Hairline Progress + Inline Spinner + Muted Subtitle + Coordinated Dual-Pane Dim
	{
		d := newDemo("history", "light")
		d.query = "release"
		l := newStaticLiveState()
		l.loaded = true
		l.foregroundLoading = true
		l.loadingSince = time.Now().Add(-360 * time.Millisecond)
		l.key = d.liveQueryKey()
		l.displayedKey = "prev-query"
		l.displayedScope = d.liveScopeKey()
		d.live = l
		d.entries = makeEvidenceEntries(fullImg, thumbImg)
		d.selectEntry("e1")
		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{})
		tt.SetScale(2)
		tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
		// Land the 1050ms hairline sweep squarely across the center of the toolbar.
		for {
			tt.Advance(40 * time.Millisecond)
			phase := float32(tt.Now().UnixNano()%(1050*time.Millisecond).Nanoseconds()) / float32((1050 * time.Millisecond).Nanoseconds())
			if phase >= 0.42 && phase <= 0.58 {
				break
			}
		}
		savePNGFile(t, filepath.Join(dir, "03-slow-query-hairline-dual-dim.png"), tt.Image())
	}

	// Shot 04: Progressive Detail Image Blur-up (Cached Thumbnail -> Full-Res + "正在加载原图…" badge)
	{
		d := newDemo("history", "light")
		l := newStaticLiveState()
		l.loaded = true
		l.detailLoadingSince = time.Now().Add(-300 * time.Millisecond)
		l.key = d.liveQueryKey()
		l.displayedKey = l.key
		d.live = l
		entries := makeEvidenceEntries(fullImg, thumbImg)
		entries[2].image = thumbImg
		entries[2].picture = &historyImage{bitmap: thumbImg, edge: 256, width: 1600, height: 1000, released: true}
		d.entries = entries
		d.selectEntry("e3")
		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{})
		tt.SetScale(2)
		tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
		tt.Advance(200 * time.Millisecond)
		savePNGFile(t, filepath.Join(dir, "04-detail-blurup-hd-badge.png"), tt.Image())
	}

	// Shot 05: Cold-Start Structural Skeleton (>120ms, Split List + Detail preserved)
	{
		d := newDemo("history", "light")
		d.entries = nil
		d.selected = ""
		l := newStaticLiveState()
		l.loaded = false
		l.foregroundLoading = true
		l.loadingSince = time.Now().Add(-350 * time.Millisecond)
		l.key = d.liveQueryKey()
		d.live = l
		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{})
		tt.SetScale(2)
		tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
		tt.Advance(200 * time.Millisecond)
		savePNGFile(t, filepath.Join(dir, "05-cold-start-structural-skeleton.png"), tt.Image())
	}

	// Shot 06: Quick Panel 300ms Debounce Stale-While-Revalidate (Rows preserved + Live Highlight + 2px Hairline)
	{
		d := newDemo("history", "light")
		l := newStaticLiveState()
		l.loaded = true
		d.live = l
		p := &quickPanelState{
			theme: "light", mode: "history", visible: true, hovered: -1, keyboard: true,
			live:  &quickLive{d: d},
			query: "docker",
			rows: []qpEntry{
				{id: "q1", title: "docker compose up -d --build && go test -race ./app/desktop/...", kind: "代码", source: "MacBook Pro", age: "18:24", favorite: true},
				{id: "q2", title: "Desktop Alpha build 4 release checklist & docker verification", kind: "文本", source: "MacBook Pro", age: "18:10"},
				{id: "q3", title: "BoardUI_Release_Spec.png", kind: "图片", source: "MacBook Pro", age: "17:52"},
				{id: "q4", title: "https://github.com/UniClipboard/uniclipboard/blob/main/docs/contracts/history-search-contract.md", kind: "链接", source: "iPhone", age: "17:15"},
				{id: "q5", title: "release-alpha-4-artifacts.pdf", kind: "文件", source: "Linux", age: "昨天"},
			},
		}
		p.now = time.Now()
		p.queryDue = p.now.Add(2 * time.Second)
		tt := ui.NewTesterWith(p.panel, 600, 420, ui.TesterOptions{})
		tt.SetScale(2)
		for {
			tt.Advance(40 * time.Millisecond)
			p.now = tt.Now()
			p.queryDue = p.now.Add(300 * time.Millisecond)
			phase := float32(tt.Now().UnixNano()%(1050*time.Millisecond).Nanoseconds()) / float32((1050 * time.Millisecond).Nanoseconds())
			if phase >= 0.42 && phase <= 0.58 {
				break
			}
		}
		savePNGFile(t, filepath.Join(dir, "06-quick-panel-debounce-stale-keep.png"), tt.Image())
	}

	// ---------------- 2. Main Desktop Animation Frames (24 fps) ----------------
	{
		framesDir := filepath.Join(dir, "frames_main")
		frameIdx := 0
		const dt = 42 * time.Millisecond

		d := newDemo("history", "light")
		d.entries = nil
		d.selected = ""
		l := newStaticLiveState()
		l.loaded = false
		l.foregroundLoading = true
		l.loadingSince = time.Now().Add(-300 * time.Millisecond)
		l.key = d.liveQueryKey()
		d.live = l

		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{})
		tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})

		record := func(n int) {
			for i := 0; i < n; i++ {
				tt.Advance(dt)
				savePNGFile(t, filepath.Join(framesDir, fmt.Sprintf("f%04d.png", frameIdx)), tt.Image())
				frameIdx++
			}
		}

		// Phase A (1.4s): Cold-start structural skeleton with animated shimmer.
		record(34)

		// Phase B (1.4s): Cold load settles into List + Detail ("cold" transition).
		allEntries := makeEvidenceEntries(fullImg, thumbImg)
		l.loaded = true
		l.foregroundLoading = false
		l.loadingSince = time.Time{}
		l.lastResultKind = "cold"
		l.displayedKey = d.liveQueryKey()
		l.displayedScope = d.liveScopeKey()
		l.resultEpoch++
		d.entries = allEntries
		d.selectEntry("e2") // User reading item #2 (code)
		record(32)

		// Phase C (1.8s): Slow search typing "release" (>120ms) — hairline glides, subtitle dims,
		// List+Detail dim in sync, NO blank wipe!
		d.query = "rel"
		l.foregroundLoading = true
		l.loadingSince = time.Now().Add(-200 * time.Millisecond)
		l.key = d.liveQueryKey()
		record(14)
		d.query = "release"
		l.key = d.liveQueryKey()
		record(30)

		// Phase D (1.6s): Refined results arrive — selection continuity keeps e2 selected!
		// Highlights update cleanly with ZERO vertical bounce.
		l.foregroundLoading = false
		l.loadingSince = time.Time{}
		l.lastResultKind = "refine"
		l.displayedKey = d.liveQueryKey()
		l.resultEpoch++
		d.entries = []entry{allEntries[0], allEntries[1], allEntries[2], allEntries[3]}
		// Keep e2 selected (same-scope continuity!)
		record(38)

		// Phase E (2.2s): Click image e3 -> Blur-up thumbnail + "正在加载原图…" badge -> sharpens!
		entriesBlur := make([]entry, len(d.entries))
		copy(entriesBlur, d.entries)
		entriesBlur[2].image = thumbImg
		entriesBlur[2].picture = &historyImage{bitmap: thumbImg, edge: 256, width: 1600, height: 1000, released: true}
		d.entries = entriesBlur
		l.detailLoadingSince = time.Now().Add(-250 * time.Millisecond)
		d.selectEntry("e3")
		record(28)

		// Full-res arrives -> smooth blur(6) -> blur(0) sharpening!
		entriesSharp := make([]entry, len(d.entries))
		copy(entriesSharp, d.entries)
		entriesSharp[2].image = fullImg
		entriesSharp[2].picture = &historyImage{bitmap: fullImg, edge: 0, width: 1600, height: 1000, released: true}
		d.entries = entriesSharp
		l.detailLoadingSince = time.Time{}
		record(26)

		// Phase F (2.2s): Switch to Grid View with highlights & shimmer card, then open 436px drawer!
		d.entries = makeEvidenceEntries(fullImg, thumbImg)
		d.layout = "grid"
		d.inspector = false
		record(26)
		d.selectEntry("e1")
		d.inspector = true
		record(28)
	}

	// ---------------- 3. Quick Panel Debounce Animation Frames (24 fps) ----------------
	{
		framesDir := filepath.Join(dir, "frames_qp")
		frameIdx := 0
		const dt = 42 * time.Millisecond

		d := newDemo("history", "light")
		l := newStaticLiveState()
		l.loaded = true
		d.live = l

		allRows := []qpEntry{
			{id: "q1", title: "Desktop Alpha build 4 release checklist", kind: "文本", source: "MacBook Pro", age: "18:24", favorite: true},
			{id: "q2", title: "docker compose up -d --build && go test -race ./app/desktop/...", kind: "代码", source: "Linux", age: "18:10"},
			{id: "q3", title: "BoardUI_Release_Spec.png", kind: "图片", source: "MacBook Pro", age: "17:52"},
			{id: "q4", title: "https://github.com/UniClipboard/uniclipboard/blob/main/docs/contracts/history-search-contract.md", kind: "链接", source: "iPhone", age: "17:15"},
			{id: "q5", title: "docker-release-checksums.txt", kind: "文件", source: "Linux", age: "昨天"},
		}
		p := &quickPanelState{
			theme: "light", mode: "history", visible: true, hovered: -1, keyboard: true,
			live: &quickLive{d: d},
			rows: allRows,
		}
		tt := ui.NewTesterWith(p.panel, 600, 420, ui.TesterOptions{})

		record := func(n int) {
			for i := 0; i < n; i++ {
				tt.Advance(dt)
				savePNGFile(t, filepath.Join(framesDir, fmt.Sprintf("f%04d.png", frameIdx)), tt.Image())
				frameIdx++
			}
		}

		// Initial open
		record(16)
		// Typing "d" -> "do" -> "doc" -> "dock" -> "docker" across 300ms debounce (rows STAY visible!)
		for _, q := range []string{"d", "do", "doc", "dock", "docker"} {
			p.query = q
			p.now = tt.Now()
			p.queryDue = p.now.Add(300 * time.Millisecond)
			record(10)
		}
		// Slow query completes after hairline sweep
		l.foregroundLoading = true
		l.loadingSince = time.Now().Add(-200 * time.Millisecond)
		p.queryDue = time.Time{}
		record(18)
		l.foregroundLoading = false
		l.loadingSince = time.Time{}
		p.rows = []qpEntry{allRows[1], allRows[4]}
		record(28)
	}
}
