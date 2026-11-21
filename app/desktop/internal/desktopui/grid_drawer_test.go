package desktopui

import (
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func TestGridA1AndGlobalDrawerD1a(t *testing.T) {
	snapDir := os.Getenv("UNIC_GRID_SNAP_DIR")
	saveSnap := func(tt *ui.Tester, name string) {
		if snapDir == "" {
			return
		}
		if err := os.MkdirAll(snapDir, 0755); err != nil {
			t.Fatal(err)
		}
		f, err := os.Create(filepath.Join(snapDir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		if err := png.Encode(f, tt.Image()); err != nil {
			_ = f.Close()
			t.Fatal(err)
		}
		if err := f.Close(); err != nil {
			t.Fatal(err)
		}
	}

	for _, theme := range []string{"light", "dark"} {
		t.Run(theme, func(t *testing.T) {
			d := newDemo("history", theme)
			d.layout = "grid"
			d.inspector = false
			tt := ui.NewTesterWith(d.view, 1180, 760, ui.TesterOptions{Dark: theme == "dark"})
			tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
			if snapDir != "" {
				tt.SetScale(2)
			}

			// 1. Grid initially shows full-width cards without the drawer open.
			if _, ok := tt.AccessByRole(ui.RoleRegion, "历史详情"); ok {
				t.Fatal("grid detail drawer must start closed")
			}
			// Source app provenance is surfaced directly on A1 cards, and the first
			// card aligns on the same 16-DIP leading rail as the toolbar and filters.
			if !tt.HasText("Google Chrome") {
				t.Fatal("expected A1 grid card to display Google Chrome source app")
			}
			gridList, ok := tt.AccessByRole(ui.RoleList, "历史网格")
			if !ok {
				t.Fatal("history grid missing")
			}
			firstCard, ok := tt.AccessByRole(ui.RoleListItem, "周一产品讨论")
			if !ok {
				t.Fatal("first grid card missing")
			}
			if got := firstCard.Bounds.X - gridList.Bounds.X; got < 15.5 || got > 16.5 {
				t.Errorf("first grid card left inset=%v, want 16 (card=%+v grid=%+v)", got, firstCard.Bounds, gridList.Bounds)
			}
			saveSnap(tt, "grid-a1-"+theme)

			// 2. Clicking the Google Chrome link card opens the D1-b floating inset full-height card with M5 motion.
			if err := tt.Click("UniClipboard"); err != nil {
				t.Fatalf("click UniClipboard card: %v", err)
			}
			// Mid-motion frame should still be animating in before 280ms settle.
			tt.Advance(80 * time.Millisecond)
			midDrawer, ok := tt.AccessByRole(ui.RoleRegion, "历史详情")
			if !ok {
				t.Fatal("global detail drawer missing during enter motion")
			}
			// Settle enter motion completely.
			tt.Advance(260 * time.Millisecond)
			drawer, ok := tt.AccessByRole(ui.RoleRegion, "历史详情")
			if !ok {
				t.Fatal("global detail drawer did not settle open after clicking grid card")
			}
			if midDrawer.Bounds.X <= drawer.Bounds.X {
				t.Errorf("expected M5 enter slide from right: midX=%v finalX=%v", midDrawer.Bounds.X, drawer.Bounds.X)
			}
			if drawer.Bounds.Y < 7.5 || drawer.Bounds.Y > 8.5 {
				t.Errorf("drawer top=%v, want 8 (floating inset covering titlebar)", drawer.Bounds.Y)
			}
			if drawer.Bounds.H < 743 || drawer.Bounds.H > 745 {
				t.Errorf("drawer height=%v, want 744 (window height - 16 inset)", drawer.Bounds.H)
			}
			if d.selected != "link" {
				t.Errorf("selected=%q, want link", d.selected)
			}
			saveSnap(tt, "grid-a1-d1b-drawer-"+theme)

			// 3. Stepping next/previous via drawer topbar pager triggers S1 content transition and settles cleanly.
			if err := tt.Click("下一条"); err != nil {
				t.Fatalf("click next in drawer: %v", err)
			}
			tt.Advance(60 * time.Millisecond)
			tt.Advance(220 * time.Millisecond)
			if d.selected == "link" {
				t.Error("clicking next did not advance selection")
			}
			if err := tt.Click("上一条"); err != nil {
				t.Fatalf("click prev in drawer: %v", err)
			}
			tt.Advance(260 * time.Millisecond)
			if d.selected != "link" {
				t.Errorf("clicking prev did not return to link, got %q", d.selected)
			}

			// 4. Closing the drawer via Close button hides it immediately from assistive tree while playing M5 exit,
			// and clicking the already-selected card re-opens it.
			if err := tt.Click("关闭详情"); err != nil {
				t.Fatalf("click close drawer: %v", err)
			}
			if _, ok := tt.AccessByRole(ui.RoleRegion, "历史详情"); ok {
				t.Fatal("drawer remained accessible after clicking close")
			}
			tt.Advance(260 * time.Millisecond)
			if err := tt.Click("UniClipboard"); err != nil {
				t.Fatalf("re-click already-selected card: %v", err)
			}
			tt.Advance(320 * time.Millisecond)
			if _, ok := tt.AccessByRole(ui.RoleRegion, "历史详情"); !ok {
				t.Fatal("re-clicking already-selected card did not reopen drawer")
			}

			// 5. Pressing Escape closes the drawer cleanly.
			tt.Key(0, ui.KeyEscape)
			tt.Advance(260 * time.Millisecond)
			if _, ok := tt.AccessByRole(ui.RoleRegion, "历史详情"); ok {
				t.Fatal("Escape did not close global drawer")
			}
		})
	}
}

func TestListL1UnifiedSearchU1AndMotionV3(t *testing.T) {
	snapDir := os.Getenv("UNIC_GRID_SNAP_DIR")
	saveSnap := func(tt *ui.Tester, name string) {
		if snapDir == "" {
			return
		}
		if err := os.MkdirAll(snapDir, 0755); err != nil {
			t.Fatal(err)
		}
		f, err := os.Create(filepath.Join(snapDir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		if err := png.Encode(f, tt.Image()); err != nil {
			_ = f.Close()
			t.Fatal(err)
		}
		if err := f.Close(); err != nil {
			t.Fatal(err)
		}
	}

	for _, theme := range []string{"light", "dark"} {
		t.Run(theme, func(t *testing.T) {
			d := newDemo("history", theme)
			d.layout = "list"
			tt := ui.NewTesterWith(d.view, 1180, 760, ui.TesterOptions{Dark: theme == "dark"})
			tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
			if snapDir != "" {
				tt.SetScale(2)
			}

			// 1. U1 centered unified omnibar is present in the 52-DIP toolbar with filter funnel button,
			// and the old separate 4-dropdown filter bar is gone.
			search, ok := tt.AccessByRole(ui.RoleSearch, "")
			if !ok || search.Bounds.W < 200 {
				t.Fatalf("U1 omnibar missing or too narrow: %+v", search.Bounds)
			}
			if _, ok := tt.AccessByRole(ui.RoleButton, "筛选条件"); !ok {
				t.Fatal("U1 integrated filter button missing")
			}
			if _, ok := tt.AccessByRole(ui.RoleComboBox, "内容类型"); ok {
				t.Fatal("legacy standalone filter select must be removed")
			}
			saveSnap(tt, "list-l1-u1-"+theme)

			// 2. Open the U1 filter popover and toggle a chip.
			if err := tt.Click("筛选条件"); err != nil {
				t.Fatalf("click filter funnel: %v", err)
			}
			if !d.filterOpen {
				t.Fatal("filter popover did not open")
			}
			saveSnap(tt, "list-l1-u1-popover-"+theme)
			if err := tt.Click("代码"); err != nil {
				t.Fatalf("click code chip: %v", err)
			}
			if d.kind != "代码" {
				t.Fatalf("kind=%q, want 代码", d.kind)
			}
			// Clear all filters from inside the popover.
			if err := tt.Click("清除全部"); err != nil {
				t.Fatalf("click clear all: %v", err)
			}
			if d.kind != "" || d.filterOpen {
				t.Fatalf("clear all did not reset state: kind=%q filterOpen=%v", d.kind, d.filterOpen)
			}

			// 3. Switch List -> Grid (V3 directional slide + smooth card-size slider reveal).
			if err := tt.Click("网格"); err != nil {
				t.Fatalf("switch to grid: %v", err)
			}
			tt.Advance(60 * time.Millisecond)
			tt.Advance(240 * time.Millisecond)
			if d.layout != "grid" {
				t.Fatalf("layout=%q, want grid", d.layout)
			}
			if _, ok := tt.AccessByRole(ui.RoleSlider, "卡片大小"); !ok {
				t.Fatal("card size slider did not reveal in grid mode")
			}
			saveSnap(tt, "grid-a1-u1-"+theme)

			// 4. Switch Grid -> List and confirm card size slider hides cleanly.
			if err := tt.Click("列表"); err != nil {
				t.Fatalf("switch back to list: %v", err)
			}
			tt.Advance(260 * time.Millisecond)
			if d.layout != "list" {
				t.Fatalf("layout=%q, want list", d.layout)
			}
			if _, ok := tt.AccessByRole(ui.RoleSlider, "卡片大小"); ok {
				t.Fatal("card size slider remained visible in list mode")
			}
		})
	}
}

func TestOtherPagesDarkAudit(t *testing.T) {
	snapDir := os.Getenv("UNIC_GRID_SNAP_DIR")
	saveSnap := func(tt *ui.Tester, name string) {
		if snapDir == "" {
			return
		}
		if err := os.MkdirAll(snapDir, 0755); err != nil {
			t.Fatal(err)
		}
		f, err := os.Create(filepath.Join(snapDir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		if err := png.Encode(f, tt.Image()); err != nil {
			_ = f.Close()
			t.Fatal(err)
		}
		if err := f.Close(); err != nil {
			t.Fatal(err)
		}
	}

	for _, page := range []string{"devices", "settings", "quick-panel"} {
		t.Run(page, func(t *testing.T) {
			d := newDemo(page, "dark")
			w, h := 1180, 760
			if page == "quick-panel" {
				w, h = 420, 420
			}
			tt := ui.NewTesterWith(d.view, w, h, ui.TesterOptions{Dark: true})
			if page != "quick-panel" {
				tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
			}
			if snapDir != "" {
				tt.SetScale(2)
			}
			tt.Frame()
			saveSnap(tt, page+"-dark")
		})
	}
}

func TestLiveProductionParityHelpers(t *testing.T) {
	// 1. projectHistory formats TotalBytes with humanBytes for readable cards and facts.
	small := projectHistory(api.HistoryItem{ID: "a", Kind: "text", TotalBytes: 42})
	if small.size != "42 B" {
		t.Fatalf("small size=%q, want 42 B", small.size)
	}
	large := projectHistory(api.HistoryItem{ID: "b", Kind: "image", TotalBytes: 286 * 1024})
	if large.size != "286.0 KB" {
		t.Fatalf("large size=%q, want 286.0 KB", large.size)
	}

	// 2. entryBodyText falls back to search preview before detail text is prefetched.
	e := entry{title: "Title", preview: "Title\nSecond line preview"}
	if got := entryBodyText(e); got != e.preview {
		t.Fatalf("entryBodyText=%q, want preview fallback", got)
	}
	if got := listRowExcerpt(e, e.title); got != "Second line preview" {
		t.Fatalf("listRowExcerpt=%q, want Second line preview", got)
	}

	// 3. Live filter popover exposes only Engine-supported types, includes catalogue sources,
	//    and generates prospective count queries.
	d := newDemo("history", "light")
	d.live = &liveState{
		catalogue: map[string][]engine.HistorySearchCandidate{
			"source": {
				{Value: "local", Label: "本机"},
				{Value: "external", Label: "外部来源"},
			},
		},
	}
	kinds := d.filterPopoverKinds()
	for _, k := range kinds {
		if k == "代码" || k == "链接" {
			t.Fatalf("live filter popover must not expose unsupported kind %q", k)
		}
	}
	foundRich := false
	for _, k := range kinds {
		if k == "富文本" {
			foundRich = true
		}
	}
	if !foundRich {
		t.Fatalf("live filter popover missing 富文本: %v", kinds)
	}
	foundExternal := false
	for _, src := range d.filterPopoverSources() {
		if src[0] == "external" {
			foundExternal = true
		}
	}
	if !foundExternal {
		t.Fatalf("live filter popover missing catalogue source external: %v", d.filterPopoverSources())
	}
	if qs := d.filterPopoverProspectiveQueries(); len(qs) == 0 {
		t.Fatal("expected prospective queries for live filter popover")
	}
}

func TestListL1HoverHasZeroLayoutJitter(t *testing.T) {
	d := newDemo("history", "dark")
	d.layout = "list"
	tt := ui.NewTesterWith(d.view, 1180, 760, ui.TesterOptions{Dark: true})
	tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})

	labels := []string{"周一产品讨论", "HTTP 服务示例", "UniClipboard", "桌面布局.png", "设计交付 · 3 个文件"}
	baseline := make(map[string]ui.Rect, len(labels))
	for _, lbl := range labels {
		a, ok := tt.AccessByRole(ui.RoleListItem, lbl)
		if !ok {
			t.Fatalf("missing list row %q", lbl)
		}
		baseline[lbl] = a.Bounds
	}

	// Hover each row in sequence and assert every row keeps exact baseline bounds (0px jitter).
	for _, target := range labels {
		b := baseline[target]
		tt.Move(b.X+b.W/2, b.Y+b.H/2)
		for _, lbl := range labels {
			a, ok := tt.AccessByRole(ui.RoleListItem, lbl)
			if !ok {
				t.Fatalf("missing list row %q while hovering %q", lbl, target)
			}
			if a.Bounds != baseline[lbl] {
				t.Fatalf("row %q jittered while hovering %q: got %+v want %+v", lbl, target, a.Bounds, baseline[lbl])
			}
		}
	}
}

func TestHistoryItemContextMenu(t *testing.T) {
	d := newDemo("history", "light")
	// Entry 0 ("note") is pinned; entry 1 ("code") is unpinned.
	pinnedMenu := d.historyItemMenu([]string{"note"})
	if len(pinnedMenu) != 5 {
		t.Fatalf("menu entries=%d, want 5", len(pinnedMenu))
	}
	if pinnedMenu[0].ID != "copy" || pinnedMenu[1].ID != "pin" || pinnedMenu[1].Label != "取消置顶" || pinnedMenu[2].ID != "send" || !pinnedMenu[3].Separator || pinnedMenu[4].ID != "delete" || !pinnedMenu[4].Destructive {
		t.Fatalf("unexpected pinned item menu: %+v", pinnedMenu)
	}
	if len(pinnedMenu[2].Children) < 2 || pinnedMenu[2].Children[0].ID != "send:all" {
		t.Fatalf("expected send submenu targets, got %+v", pinnedMenu[2].Children)
	}

	unpinnedMenu := d.historyItemMenu([]string{"code"})
	if unpinnedMenu[1].Label != "置顶" {
		t.Fatalf("unpinned label=%q, want 置顶", unpinnedMenu[1].Label)
	}

	// Verify right-click context menu works in both list and grid layouts.
	for _, layout := range []string{"list", "grid"} {
		t.Run(layout, func(t *testing.T) {
			d := newDemo("history", "light")
			d.layout = layout
			tt := ui.NewTesterWith(d.view, 1180, 760, ui.TesterOptions{})
			target, ok := tt.AccessByRole(ui.RoleListItem, "HTTP 服务示例")
			if !ok {
				t.Fatal("missing HTTP 服务示例 item")
			}
			tt.RightClickAt(target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2)
			if err := tt.Click("置顶"); err != nil {
				t.Fatalf("click pin from context menu: %v", err)
			}
			if !d.entries[1].pinned {
				t.Fatal("context menu pin did not pin HTTP 服务示例")
			}
			tt.RightClickAt(target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2)
			if err := tt.Click("复制"); err != nil {
				t.Fatalf("click copy from context menu: %v", err)
			}
			if d.notice == "" {
				t.Fatal("context menu copy did not trigger copy notice")
			}
			tt.RightClickAt(target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2)
			if err := tt.Click("删除"); err != nil {
				t.Fatalf("click delete from context menu: %v", err)
			}
			if d.confirm == "" {
				t.Fatal("context menu delete did not open confirmation dialog")
			}
		})
	}
}
