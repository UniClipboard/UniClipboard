package desktopui

import (
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/windowchrome"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"
)

func attachWindowsControls(d *viewState) {
	actions := windowchrome.Actions{
		Minimize:       func() {},
		ToggleMaximize: func() {},
		Close:          func() {},
		IsMaximized:    func() bool { return false },
	}
	d.toolbarTrailing = func(c *ui.Context) {
		windowchrome.Controls(c, actions, d.currentLanguage())
	}
}

func assertToolbarNoOverlap(t *testing.T, tt *ui.Tester, toolbarTitle string, width float32) []ui.AccessInfo {
	t.Helper()
	return assertToolbarNoOverlapScaled(t, tt, toolbarTitle, width, 1)
}

func assertToolbarNoOverlapScaled(t *testing.T, tt *ui.Tester, toolbarTitle string, width, scale float32) []ui.AccessInfo {
	t.Helper()
	bar, ok := tt.AccessByRole(ui.RoleToolbar, toolbarTitle)
	if !ok {
		t.Fatalf("missing toolbar %q", toolbarTitle)
	}
	wantH := 52 * scale
	if bar.Bounds.H < wantH-0.2 || bar.Bounds.H > wantH+0.2 {
		t.Errorf("toolbar height=%v, want %v", bar.Bounds.H, wantH)
	}
	inset := 6 * scale
	var controls []ui.AccessInfo
	for _, n := range tt.AccessAll() {
		if n.Bounds.W <= 0 || n.Bounds.H <= 0 {
			continue
		}
		if n.Bounds.Y >= bar.Bounds.Y+bar.Bounds.H-0.1 || n.Bounds.Y+n.Bounds.H <= bar.Bounds.Y+0.1 {
			continue
		}
		if n.Role == ui.RoleToolbar {
			continue
		}
		// Keep top-level toolbar controls (avoid double-counting child buttons inside search or segmented group).
		if n.Role != ui.RoleSearch &&
			n.Role != ui.RoleGroup &&
			n.Name != "切换侧栏" &&
			n.Name != "返回历史" &&
			n.Name != "设备" &&
			n.Name != "发送到设备" &&
			n.Name != "切换收藏" &&
			n.Name != "动作" &&
			n.Name != "更多操作" &&
			n.Name != "收起搜索" &&
			n.Name != "刷新状态" &&
			n.Name != "添加设备" &&
			n.Name != "最小化" &&
			n.Name != "最大化" &&
			n.Name != "还原" &&
			n.Name != "关闭" {
			continue
		}
		b := n.Bounds
		if b.X < bar.Bounds.X-0.1 || b.X+b.W > width+0.1 || b.Y < bar.Bounds.Y+inset || b.Y+b.H > bar.Bounds.Y+bar.Bounds.H-inset+0.1 {
			t.Errorf("control %v(%q) out of toolbar bounds: control=%+v toolbar=%+v", n.Role, n.Name, b, bar.Bounds)
		}
		controls = append(controls, n)
	}
	slices.SortFunc(controls, func(a, b ui.AccessInfo) int {
		if a.Bounds.X < b.Bounds.X {
			return -1
		}
		if a.Bounds.X > b.Bounds.X {
			return 1
		}
		return 0
	})
	for i := 1; i < len(controls); i++ {
		prev, cur := controls[i-1], controls[i]
		if prev.Bounds.X+prev.Bounds.W > cur.Bounds.X+0.1 {
			t.Errorf("toolbar controls overlap: %v(%q)=%+v overlaps %v(%q)=%+v", prev.Role, prev.Name, prev.Bounds, cur.Role, cur.Name, cur.Bounds)
		}
	}
	return controls
}

func saveOptionalCapture(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("UNIC_NARROW_TITLEBAR_CAPTURES")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0755); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name+".png"))
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

func TestNarrowTitlebarWindowsAllActionsAndDetail(t *testing.T) {
	for _, theme := range []string{"dark", "light"} {
		for _, width := range []int{600, 640, 700, 800, 1100} {
			t.Run(theme+"/"+string(rune('0'+width/1000))+string(rune('0'+(width/100)%10))+string(rune('0'+(width/10)%10))+"0", func(t *testing.T) {
				d := newDemo("history", theme)
				attachWindowsControls(d)
				// Populate live actions so the 4th toolbar command ("动作" pencil icon) renders,
				// matching the exact crowded production Windows titlebar.
				l := newStaticLiveState()
				l.loaded = true
				l.key = d.liveQueryKey()
				l.displayedKey = l.key
				l.actions = []api.Action{
					{PluginID: "builtin.text", Key: "upper", Title: "转大写", Accepts: []string{"text"}, Placements: []string{plugin.ActionPlacementHistoryToolbar}},
				}
				l.items = map[string]api.HistoryItem{
					d.entries[0].id: {ID: d.entries[0].id, Kind: "text"},
				}
				d.live = l

				tt := ui.NewTesterWith(d.view, width, 480, ui.TesterOptions{Dark: theme == "dark"})
				tt.SetScale(2)
				assertToolbarNoOverlap(t, tt, "全部", float32(width))
				if width == 600 {
					saveOptionalCapture(t, tt, "win-600-"+theme+"-idle-4cmds")
					// In narrow mode, search starts collapsed as a button trigger (not combobox).
					if _, ok := tt.AccessByRole(ui.RoleComboBox, "搜索历史"); ok {
						t.Fatal("expected narrow 600px Windows titlebar to collapse search at idle")
					}
					if _, ok := tt.AccessByRole(ui.RoleButton, "搜索历史"); !ok {
						t.Fatal("expected collapsed search trigger button in narrow idle toolbar")
					}
					if _, ok := tt.AccessByRole(ui.RoleMenuButton, "动作"); !ok {
						t.Fatal("expected 4th command button (动作) visible in narrow idle toolbar")
					}

					// Open single-pane detail view (adds Back icon button) and verify still zero overlap.
					d.detailOpen = true
					tt.Frame()
					assertToolbarNoOverlap(t, tt, "全部", float32(width))
					saveOptionalCapture(t, tt, "win-600-"+theme+"-detail-back")
					d.detailOpen = false
					tt.Frame()
				}
			})
		}
	}
}

func TestNarrowTitlebarSearchTakeoverInteraction(t *testing.T) {
	d := newDemo("history", "dark")
	attachWindowsControls(d)
	tt := ui.NewTesterWith(d.view, 600, 480, ui.TesterOptions{Dark: true})
	tt.SetScale(2)

	// 1. Starts in Narrow Idle: search trigger button visible, commands visible, zero overlap.
	assertToolbarNoOverlap(t, tt, "全部", 600)
	if _, ok := tt.AccessByRole(ui.RoleButton, "搜索历史"); !ok {
		t.Fatal("expected search trigger button at 600px Windows idle")
	}

	// 2. Clicking the search trigger enters Search Takeover and focuses the omnibar.
	if err := tt.Click("搜索历史"); err != nil {
		t.Fatal(err)
	}
	tt.Advance(200 * time.Millisecond)
	assertToolbarNoOverlap(t, tt, "全部", 600)
	saveOptionalCapture(t, tt, "win-600-dark-takeover")
	input, ok := tt.AccessByRole(ui.RoleComboBox, "搜索历史")
	if !ok {
		t.Fatal("expected search combobox during narrow search takeover")
	}
	searchBox, ok := tt.AccessByRole(ui.RoleSearch, "")
	if !ok || searchBox.Bounds.W < 340 {
		t.Fatalf("expected wide takeover omnibar (>=340 DIP), got %+v", searchBox.Bounds)
	}
	filterBtn, ok := tt.AccessByRole(ui.RoleButton, "筛选条件")
	if !ok {
		t.Fatal("filter funnel button must be visible during narrow search takeover")
	}
	if filterBtn.Bounds.X+filterBtn.Bounds.W > searchBox.Bounds.X+searchBox.Bounds.W+0.5 {
		t.Fatalf("filter button clipped outside search box: btn=%+v box=%+v", filterBtn.Bounds, searchBox.Bounds)
	}
	if input.Bounds.W < 180 {
		t.Fatalf("expected comfortable input width in takeover, got %+v", input.Bounds)
	}
	if active, _ := tt.TextInputActive(); !active {
		t.Fatal("clicking search trigger did not focus input")
	}

	// 3. Type query, then open Filter popover inside takeover: takeover must stay open.
	tt.Type("HTTP 服务示例")
	if d.query != "HTTP 服务示例" || len(d.filtered()) != 1 {
		t.Fatalf("expected query HTTP 服务示例 -> 1 match, got query=%q len=%d", d.query, len(d.filtered()))
	}
	if err := tt.Click("筛选条件"); err != nil {
		t.Fatal(err)
	}
	if !d.filterOpen || !d.narrowSearchOpen {
		t.Fatalf("expected filter popover & takeover open: filterOpen=%v narrowSearchOpen=%v", d.filterOpen, d.narrowSearchOpen)
	}
	saveOptionalCapture(t, tt, "win-600-dark-takeover-filter")
	chip, ok := tt.AccessByRole(ui.RoleButton, "代码")
	if !ok {
		t.Fatal("filter chip 代码 missing")
	}
	tt.ClickAt(chip.Bounds.X+chip.Bounds.W/2, chip.Bounds.Y+chip.Bounds.H/2)
	if d.kind != "代码" || !d.narrowSearchOpen {
		t.Fatalf("filter chip did not keep takeover open: kind=%q narrowSearchOpen=%v", d.kind, d.narrowSearchOpen)
	}

	// 4. Clicking collapse button exits takeover while preserving query & filter, showing active indicator.
	if err := tt.Click("收起搜索"); err != nil {
		t.Fatal(err)
	}
	tt.Advance(200 * time.Millisecond)
	if d.narrowSearchOpen || d.filterOpen {
		t.Fatalf("collapse button did not close takeover/filter: narrowSearchOpen=%v filterOpen=%v", d.narrowSearchOpen, d.filterOpen)
	}
	if d.query != "HTTP 服务示例" || d.kind != "代码" {
		t.Fatalf("collapsing narrow takeover lost active search/filter: query=%q kind=%q", d.query, d.kind)
	}
	assertToolbarNoOverlap(t, tt, "全部", 600)
	saveOptionalCapture(t, tt, "win-600-dark-active-pill")
	// Commands (list/grid, pin, more) are immediately reachable while search/filter stays active!
	if _, ok := tt.AccessByRole(ui.RoleButton, "切换收藏"); !ok {
		t.Fatal("pin command should be reachable after collapsing takeover with active filter")
	}

	// 5. Cmd/Ctrl+F re-opens takeover with existing query & filter intact.
	tt.Key(ui.Cmd, ui.KeyF)
	tt.Advance(200 * time.Millisecond)
	if !d.narrowSearchOpen {
		t.Fatal("Cmd+F did not reopen narrow search takeover")
	}
	if active, _ := tt.TextInputActive(); !active {
		t.Fatal("Cmd+F did not focus input in takeover")
	}

	// 6. Escape chain: first Escape clears query & filters (stays in takeover), second Escape collapses takeover.
	tt.Key(0, ui.KeyEscape)
	if d.query != "" || d.kind != "" {
		t.Fatalf("first Escape should clear active query/filter: query=%q kind=%q", d.query, d.kind)
	}
	if !d.narrowSearchOpen {
		t.Fatal("first Escape clearing query should keep search takeover open")
	}
	tt.Key(0, ui.KeyEscape)
	tt.Advance(200 * time.Millisecond)
	if d.narrowSearchOpen {
		t.Fatal("second Escape on empty search should collapse takeover")
	}
	assertToolbarNoOverlap(t, tt, "全部", 600)

	// 7. Re-enter takeover, type query, click a list item (blur): collapses takeover while preserving query!
	if err := tt.Click("搜索历史"); err != nil {
		t.Fatal(err)
	}
	tt.Advance(200 * time.Millisecond)
	tt.Type("HTTP")
	if err := tt.Click("HTTP 服务示例"); err != nil {
		t.Fatal(err)
	}
	tt.Advance(200 * time.Millisecond)
	if d.narrowSearchOpen {
		t.Fatal("clicking list item outside search should collapse narrow takeover")
	}
	if d.query != "HTTP" {
		t.Fatalf("blur collapse should preserve query, got %q", d.query)
	}

	// 8. Resize continuity: Wide (focused) -> Narrow preserves Search Takeover; Narrow -> Wide keeps query.
	tt.SetSize(1100, 720)
	tt.Frame()
	if err := tt.Click("搜索历史"); err != nil {
		t.Fatal(err)
	}
	if active, _ := tt.TextInputActive(); !active {
		t.Fatal("expected wide search focused")
	}
	tt.SetSize(600, 480)
	tt.Frame()
	if !d.narrowSearchOpen {
		t.Fatal("shrinking from focused wide search to narrow should preserve search takeover")
	}
	tt.SetSize(1100, 720)
	tt.Frame()
	if d.narrowSearchOpen {
		t.Fatal("widening back to 1100 should leave takeover mode")
	}
	if d.query != "HTTP" {
		t.Fatalf("widening lost query: %q", d.query)
	}
}

func TestNarrowTitlebarUltraNarrowZoomOverflow(t *testing.T) {
	d := newDemo("history", "dark")
	attachWindowsControls(d)
	// Zoom to 150% on a 600px window -> scaled width = 400 -> availW ~ 232 < ToolbarUltraNarrowWidth (360).
	d.zoomStep = 5
	tt := ui.NewTesterWith(d.view, 600, 480, ui.TesterOptions{Dark: true})
	assertToolbarNoOverlapScaled(t, tt, "全部", 600, zoomScale(d.zoomStep))
	// Secondary commands fold into "更多操作" so nothing overflows.
	if _, ok := tt.AccessByRole(ui.RoleButton, "切换收藏"); ok {
		t.Fatal("expected 切换收藏 folded into overflow menu in ultra-narrow zoomed toolbar")
	}
	if err := tt.Click("更多操作"); err != nil {
		t.Fatal(err)
	}
	if !tt.HasText("发送到设备") || !tt.HasText("管理标签") {
		t.Fatalf("expected folded commands inside 更多操作 menu, texts=%v", tt.Texts())
	}
}

func TestNarrowTitlebarDevicesAndMacIntegrated(t *testing.T) {
	t.Run("devices-windows-600", func(t *testing.T) {
		for _, theme := range []string{"dark", "light"} {
			d := newDemo("devices", theme)
			attachWindowsControls(d)
			tt := ui.NewTesterWith(d.view, 600, 480, ui.TesterOptions{Dark: theme == "dark"})
			tt.SetScale(2)
			assertToolbarNoOverlap(t, tt, "设备管理", 600)
			saveOptionalCapture(t, tt, "devices-win-600-"+theme)
			if _, ok := tt.AccessByRole(ui.RoleButton, "返回历史"); !ok {
				t.Fatal("back button missing in narrow devices toolbar")
			}
			if _, ok := tt.AccessByRole(ui.RoleButton, "刷新状态"); !ok {
				t.Fatal("refresh button missing in narrow devices toolbar")
			}
			if _, ok := tt.AccessByRole(ui.RoleButton, "添加设备"); !ok {
				t.Fatal("add device button missing in narrow devices toolbar")
			}
		}
	})

	if runtime.GOOS == "darwin" {
		t.Run("mac-integrated-600", func(t *testing.T) {
			d := newDemo("history", "dark")
			tt := ui.NewTesterWith(d.view, 600, 480, ui.TesterOptions{Dark: true})
			tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
			tt.SetScale(2)
			assertToolbarNoOverlap(t, tt, "全部", 600)
			if _, ok := tt.AccessByRole(ui.RoleButton, "搜索历史"); !ok {
				t.Fatal("expected narrow search trigger on 600px macOS integrated titlebar")
			}
			if err := tt.Click("搜索历史"); err != nil {
				t.Fatal(err)
			}
			tt.Advance(200 * time.Millisecond)
			assertToolbarNoOverlap(t, tt, "全部", 600)
			saveOptionalCapture(t, tt, "mac-600-dark-takeover")
			searchBox, ok := tt.AccessByRole(ui.RoleSearch, "")
			if !ok || searchBox.Bounds.X < 120 || searchBox.Bounds.W < 360 {
				t.Fatalf("macOS takeover search must clear traffic lights and span toolbar: %+v", searchBox.Bounds)
			}
			// Verify ToolbarLayout thresholds directly.
			if desktopkit.ToolbarNarrowWidth <= 0 {
				t.Fatal("unexpected threshold")
			}
		})
	}
}
