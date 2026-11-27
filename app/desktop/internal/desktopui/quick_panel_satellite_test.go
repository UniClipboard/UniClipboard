package desktopui

import (
	"context"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func TestQuickPanelSatelliteSizeFitsWrappedContent(t *testing.T) {
	const prodRecord = "修复 UniClipboard/UniClipboard PR #2319 失败的 CI 检查：https://github.com/UniClipboard/UniClipboard/pull/2319"

	cases := []struct {
		name       string
		entry      qpEntry
		textZoom   int
		windowZoom int
		live       bool
	}{
		{
			name: "production-single-line-cjk-and-url",
			entry: qpEntry{
				id:     "prod-2319",
				title:  prodRecord,
				text:   prodRecord,
				kind:   "文本",
				source: "本机",
				age:    "昨天 18:50",
			},
		},
		{
			name: "production-record-text-zoom-up",
			entry: qpEntry{
				id:     "prod-2319-zoom",
				title:  prodRecord,
				text:   prodRecord,
				kind:   "文本",
				source: "本机",
				age:    "昨天 18:50",
			},
			textZoom: 3,
		},
		{
			name: "production-record-narrow-window-zoom",
			entry: qpEntry{
				id:     "prod-2319-narrow",
				title:  prodRecord,
				text:   prodRecord,
				kind:   "文本",
				source: "本机",
				age:    "昨天 18:50",
			},
			windowZoom: -2,
		},
		{
			name: "wrapped-code-entry",
			entry: qpEntry{
				id:     "code-long",
				title:  "func syncWindow",
				text:   "func syncWindow(width int, height int, anchor Rectangle) error { return applySatelliteBounds(width, height, anchor, true) }",
				kind:   "代码",
				source: "本机",
				age:    "刚刚",
			},
		},
		{
			name: "wrapped-link-entry",
			entry: qpEntry{
				id:     "link-long",
				title:  "UniClipboard Pull Request #2319: Remove inline status alert component from Desktop quick panel",
				text:   "https://github.com/UniClipboard/UniClipboard/pull/2319/files?diff=split&w=1#diff-quick-panel-window",
				kind:   "链接",
				source: "本机",
				age:    "昨天 18:50",
			},
		},
		{
			name: "wrapped-file-list",
			entry: qpEntry{
				id:     "files-long",
				title:  "files",
				text:   "/Users/mark/workspace/UniClipboard/app/desktop/internal/desktopui/quick_panel_window.go\n/Users/mark/workspace/UniClipboard/app/desktop/internal/desktopui/quick_panel_view.go",
				kind:   "文件",
				source: "本机",
				age:    "刚刚",
			},
		},
		{
			name: "short-single-line-min-height",
			entry: qpEntry{
				id:     "short",
				title:  "hello",
				text:   "hello",
				kind:   "文本",
				source: "本机",
				age:    "刚刚",
			},
		},
		{
			name: "unbroken-long-token",
			entry: qpEntry{
				id:     "token",
				title:  "jwt",
				text:   strings.Repeat("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9", 4),
				kind:   "文本",
				source: "本机",
				age:    "刚刚",
			},
		},
		{
			name: "trailing-newlines",
			entry: qpEntry{
				id:     "trailing-nl",
				title:  "line 1",
				text:   "第一行内容\n第二行内容\n\n",
				kind:   "文本",
				source: "本机",
				age:    "刚刚",
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			p := &quickPanelState{
				theme:      "light",
				mode:       "history",
				visible:    true,
				keyboard:   true,
				hovered:    -1,
				textZoom:   tc.textZoom,
				windowZoom: tc.windowZoom,
				entries:    []qpEntry{tc.entry},
				rows:       []qpEntry{tc.entry},
			}
			var gotW, gotH int
			p.onUpdate = func() {
				gotW, gotH = p.satelliteSize()
				p.satelliteWidth, p.satelliteHeight = gotW, gotH
			}
			panel := ui.NewTesterWith(p.panel, qpPanelWidth, qpPanelHeight, ui.TesterOptions{Dark: false})
			panel.Advance(510 * time.Millisecond)
			if p.previewID != tc.entry.id {
				t.Fatalf("previewID = %q, want %q", p.previewID, tc.entry.id)
			}
			if gotW <= 0 || gotH <= 0 {
				t.Fatalf("unexpected satellite size %dx%d", gotW, gotH)
			}
			if tc.windowZoom == 0 && gotH < 100 {
				t.Fatalf("satellite height %d at default zoom is below minimum native frame floor 100", gotH)
			}

			sat := ui.NewTesterWith(p.satellite, gotW, gotH, ui.TesterOptions{Dark: false})
			sat.Frame()
			// If satellite refined its size, apply and re-check.
			if gotW != p.satelliteWidth || gotH != p.satelliteHeight {
				sat.SetSize(p.satelliteWidth, p.satelliteHeight)
				sat.Frame()
				gotW, gotH = p.satelliteWidth, p.satelliteHeight
			}

			probe := tc.entry.text
			if tc.entry.kind == "文件" {
				parts := strings.Split(tc.entry.text, "\n")
				probe = parts[len(parts)-1]
			}
			before, ok := sat.Find(probe)
			if !ok {
				t.Fatalf("could not find preview text %q in satellite", probe)
			}
			bottomEdge := before.Y + before.H + qpSatelliteBodyPadBot + qpSatelliteBorder
			if bottomEdge > float32(gotH)+1 {
				t.Fatalf("preview content clipped: text bottom+padding=%.1f > satellite height=%d (text rect=%+v)", bottomEdge, gotH, before)
			}

			// Verify scroll container has zero vertical overflow in either scroll direction.
			for _, dy := range []float32{200, -200} {
				sat.Scroll(float32(gotW)/2, float32(gotH)/2, 0, dy)
				sat.Frame()
				after, ok := sat.Find(probe)
				if !ok {
					t.Fatalf("lost preview text after scroll dy=%v", dy)
				}
				if after.Y != before.Y {
					t.Fatalf("satellite overflowed and scrolled (dy=%v): before.Y=%.1f after.Y=%.1f (height=%d, text rect=%+v)", dy, before.Y, after.Y, gotH, before)
				}
			}
		})
	}
}

func TestQuickPanelSatelliteMenuFitsWithoutOverflow(t *testing.T) {
	for _, textZoom := range []int{0, 3} {
		p := newQuickDemo("light", "history")
		p.textZoom = textZoom
		var gotW, gotH int
		p.onUpdate = func() {
			gotW, gotH = p.satelliteSize()
			p.satelliteWidth, p.satelliteHeight = gotW, gotH
		}
		panel := ui.NewTesterWith(p.panel, qpPanelWidth, qpPanelHeight, ui.TesterOptions{Dark: false})
		panel.Advance(510 * time.Millisecond)
		p.openActions()
		panel.Frame()
		sat := ui.NewTesterWith(p.satellite, gotW, gotH, ui.TesterOptions{Dark: false})
		sat.Frame()
		if gotW != p.satelliteWidth || gotH != p.satelliteHeight {
			sat.SetSize(p.satelliteWidth, p.satelliteHeight)
			sat.Frame()
			gotW, gotH = p.satelliteWidth, p.satelliteHeight
		}
		actions := p.actions()
		lastLabel := p.actionLabel(actions[len(actions)-1])
		before, ok := sat.Find(lastLabel)
		if !ok {
			t.Fatalf("textZoom=%d: missing last action %q", textZoom, lastLabel)
		}
		for _, dy := range []float32{200, -200} {
			sat.Scroll(float32(gotW)/2, float32(gotH)/2, 0, dy)
			sat.Frame()
			after, ok := sat.Find(lastLabel)
			if !ok || after.Y != before.Y {
				t.Fatalf("textZoom=%d: actions menu scrolled (dy=%v): before=%v after=%v (height=%d)", textZoom, dy, before, after, gotH)
			}
		}
	}
}

func TestQuickPanelSatelliteSizeClampsHugeTextAndFollowsPreviewID(t *testing.T) {
	short := qpEntry{id: "short", title: "hi", text: "hi", kind: "文本", source: "本机", age: "刚刚"}
	huge := qpEntry{id: "huge", title: "huge", text: strings.Repeat("这是一段非常长的剪贴板文本内容用于验证高度上限。", 4000), kind: "文本", source: "本机", age: "刚刚"}
	multi := qpEntry{id: "multi", title: "multi", text: strings.Repeat("第 1 行内容\n", 10), kind: "文本", source: "本机", age: "刚刚"}

	p := &quickPanelState{
		theme:    "light",
		mode:     "history",
		visible:  true,
		keyboard: true,
		hovered:  -1,
		entries:  []qpEntry{multi, short, huge},
		rows:     []qpEntry{multi, short, huge},
	}
	var gotW, gotH int
	p.onUpdate = func() {
		gotW, gotH = p.satelliteSize()
		p.satelliteWidth, p.satelliteHeight = gotW, gotH
	}
	panel := ui.NewTesterWith(p.panel, qpPanelWidth, qpPanelHeight, ui.TesterOptions{Dark: false})
	panel.Advance(510 * time.Millisecond)
	multiH := gotH
	if p.previewID != "multi" || multiH <= qpSatelliteMinHeight {
		t.Fatalf("expected multi preview taller than min height, previewID=%q height=%d", p.previewID, multiH)
	}

	// Move selection to short item, but stay within the 120ms preview delay:
	// satellite is still showing "multi" and must keep sizing for previewID ("multi"), not active ("short").
	p.selectRow(1)
	panel.Frame()
	if p.previewID != "multi" {
		t.Fatalf("preview switched too early: %q", p.previewID)
	}
	if gotH != multiH {
		t.Fatalf("satellite resized to active() before previewID changed: got %d, want %d", gotH, multiH)
	}

	// Select huge text and wait for preview to switch: height must clamp cleanly to qpSatelliteMaxHeight
	// and remain scrollable.
	p.selectRow(2)
	panel.Advance(150 * time.Millisecond)
	if p.previewID != "huge" {
		t.Fatalf("preview did not switch to huge: %q", p.previewID)
	}
	if gotW != qpSatellitePreviewWidth || gotH != qpSatelliteMaxHeight {
		t.Fatalf("huge text satellite size = %dx%d, want %dx%d", gotW, gotH, qpSatellitePreviewWidth, qpSatelliteMaxHeight)
	}
	sat := ui.NewTesterWith(p.satellite, gotW, gotH, ui.TesterOptions{Dark: false})
	sat.Frame()
	before, ok := sat.Find(huge.text)
	if !ok {
		t.Fatal("missing huge text in satellite")
	}
	scrolled := false
	for _, dy := range []float32{200, -200} {
		sat.Scroll(float32(gotW)/2, float32(gotH)/2, 0, dy)
		sat.Frame()
		if after, ok := sat.Find(huge.text); ok && after.Y != before.Y {
			scrolled = true
			break
		}
	}
	if !scrolled {
		t.Fatalf("expected huge text satellite to be scrollable (height=%d, rect=%+v)", gotH, before)
	}
}

func TestQuickPanelExitFavoritesResetsSelectionAndAnchor(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	client := api.NewClient(filepath.Join(t.TempDir(), "unused.sock"))
	q := newLiveQuick(ctx, client, "light", nil)
	defer q.live.d.live.cancel()
	// Keep poll() from spawning background RPCs against the unused socket.
	q.live.d.live.loading = true

	allEntries := testQuickEntries(30, "hist")
	allEntries[25].pinned = true
	allEntries[25].title = "NixOS/nixpkgs"
	allEntries[25].text = "NixOS/nixpkgs"

	// Step 1-2: enter Favorites with the old record selected at row 0.
	q.favoriteOnly = true
	favKey := q.live.d.liveQueryKey()
	q.live.d.entries = []entry{allEntries[25]}
	q.live.d.live.loaded = true
	q.live.d.live.displayedKey = favKey
	q.live.d.live.appliedSerial = 1

	var syncedBounds []mygo.Rectangle
	q.onUpdate = func() {
		if q.menu == "" && q.previewEntry() == nil {
			q.satelliteWidth, q.satelliteHeight = 0, 0
			q.satelliteAnchor = mygo.Rectangle{}
			return
		}
		q.satelliteWidth, q.satelliteHeight = q.satelliteSize()
		bounds := q.satelliteAnchorBounds(qpPanelWidth, qpPanelHeight)
		q.satelliteAnchor = bounds
		syncedBounds = append(syncedBounds, bounds)
	}

	tt := ui.NewTesterWith(q.panel, qpPanelWidth, qpPanelHeight, ui.TesterOptions{Dark: false})
	tt.Advance(510 * time.Millisecond)
	if q.selected != 0 || q.active() == nil || q.active().id != "hist-25" {
		t.Fatalf("favorites selection = (%d, %v), want (0, hist-25)", q.selected, q.active())
	}
	if q.previewID != "hist-25" {
		t.Fatalf("favorites previewID = %q, want hist-25", q.previewID)
	}

	// Step 3: exit Favorites. Render an intermediate frame while the "All" query
	// is still in flight (d.entries and displayedKey still belong to Favorites).
	q.toggleFavoriteFilter()
	// Cancel the synthetic RPC against unused.sock and keep loading=true so
	// the test controls exact delivery timing deterministically.
	q.live.d.live.invalidateSearch()
	q.live.d.live.loading = true
	tt.Frame()
	if len(q.rows) != 0 {
		t.Fatalf("stale favorites rows leaked while exiting favorites: len=%d first=%+v", len(q.rows), q.rows[0])
	}
	if q.previewEntry() != nil {
		t.Fatalf("satellite preview stayed open for stale entry %+v while exiting favorites", q.previewEntry())
	}

	// Now deliver the "All" results where hist-25 sits far below the viewport.
	q.live.d.entries = allEntries
	q.live.d.live.displayedKey = q.live.d.liveQueryKey()
	q.live.d.live.appliedSerial = 2
	syncedBounds = nil

	tt.Frame()
	tt.Advance(510 * time.Millisecond)

	if q.selected != 0 || q.active() == nil || q.active().id != "hist-00" {
		t.Fatalf("post-exit selection = (%d, %v), want (0, hist-00)", q.selected, q.active())
	}
	if q.previewID != "hist-00" {
		t.Fatalf("post-exit previewID = %q, want hist-00", q.previewID)
	}
	const headerBottom = 48
	const footerTop = qpPanelHeight - 34
	row0, ok := tt.AccessByRole(ui.RoleButton, "历史：hist item 00")
	if !ok {
		t.Fatal("row 0 missing after exiting favorites")
	}
	if row0.Bounds.Y < headerBottom || row0.Bounds.Y+row0.Bounds.H > footerTop {
		t.Fatalf("row 0 bounds %+v outside visible viewport [%d, %d]", row0.Bounds, headerBottom, footerTop)
	}
	if len(syncedBounds) == 0 {
		t.Fatal("expected satellite anchor sync after exiting favorites")
	}
	last := syncedBounds[len(syncedBounds)-1]
	if last.Y < headerBottom || last.Y+last.Height > footerTop {
		t.Fatalf("satellite anchor %+v outside visible viewport [%d, %d]", last, headerBottom, footerTop)
	}
}

func TestQuickPanelSatelliteAnchorClampsOffscreenSelectedRow(t *testing.T) {
	entries := make([]qpEntry, 30)
	for i := range entries {
		entries[i] = qpEntry{
			id:     "e-" + string(rune('a'+i/26)) + string(rune('a'+i%26)),
			title:  "item " + string(rune('A'+i%26)),
			text:   "preview text",
			kind:   "文本",
			source: "本机",
			age:    "刚刚",
		}
	}
	p := &quickPanelState{
		theme:    "light",
		mode:     "history",
		visible:  true,
		keyboard: true,
		hovered:  -1,
		entries:  entries,
		rows:     entries,
	}
	var lastBounds mygo.Rectangle
	updates := 0
	p.onUpdate = func() {
		updates++
		p.satelliteWidth, p.satelliteHeight = p.satelliteSize()
		lastBounds = p.satelliteAnchorBounds(qpPanelWidth, qpPanelHeight)
		p.satelliteAnchor = lastBounds
	}

	tt := ui.NewTesterWith(p.panel, qpPanelWidth, qpPanelHeight, ui.TesterOptions{Dark: false})
	tt.Advance(510 * time.Millisecond)

	const headerBottom = 48
	const footerTop = qpPanelHeight - 34
	if lastBounds.Y < headerBottom || lastBounds.Y+lastBounds.Height > footerTop {
		t.Fatalf("initial anchor %+v outside [%d, %d]", lastBounds, headerBottom, footerTop)
	}

	// Scroll row 0 far above the viewport while keeping it selected.
	for range 6 {
		tt.Scroll(qpPanelWidth/2, qpPanelHeight/2, 0, 240)
		tt.Frame()
	}
	if lastBounds.Y < headerBottom || lastBounds.Y+lastBounds.Height > footerTop {
		t.Fatalf("anchor after scrolling selected row above viewport = %+v, want inside [%d, %d]", lastBounds, headerBottom, footerTop)
	}

	// Select row 25, scroll it into view, then scroll the list back to top so
	// row 25 sits far below the panel; the anchor must remain clamped inside the viewport.
	p.selectRow(25)
	tt.Frame()
	tt.Advance(150 * time.Millisecond)
	for range 6 {
		tt.Scroll(qpPanelWidth/2, qpPanelHeight/2, 0, -240)
		tt.Frame()
	}
	if lastBounds.Y < headerBottom || lastBounds.Y+lastBounds.Height > footerTop {
		t.Fatalf("anchor after scrolling selected row below viewport = %+v, want inside [%d, %d]", lastBounds, headerBottom, footerTop)
	}

	// Direct guard against extreme raw anchor coordinates (e.g. off-screen row or zoom).
	p.anchor = ui.Rect{Y: 950, H: 30}
	if b := p.satelliteAnchorBounds(qpPanelWidth, qpPanelHeight); b.Y < headerBottom || b.Y+b.Height > footerTop {
		t.Fatalf("extreme positive anchor not clamped: %+v", b)
	}
	p.anchor = ui.Rect{Y: -400, H: 30}
	if b := p.satelliteAnchorBounds(qpPanelWidth, qpPanelHeight); b.Y < headerBottom || b.Y+b.Height > footerTop {
		t.Fatalf("extreme negative anchor not clamped: %+v", b)
	}
	if updates == 0 {
		t.Fatal("expected satellite updates")
	}
}
