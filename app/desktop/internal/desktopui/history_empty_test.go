package desktopui

import (
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

func saveEmptyShot(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("UNIC_EMPTY_STATE_SHOTS")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
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

func TestHistoryEmptyStatesAndToolbar(t *testing.T) {
	t.Run("fresh-empty-light-and-dark", func(t *testing.T) {
		for _, theme := range []string{"light", "dark"} {
			d := newDemo("history", theme)
			d.entries = nil
			d.devices = []device{{id: "local", name: "本机", online: true}}
			tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{Dark: theme == "dark"})
			tt.SetScale(2)
			tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})

			if !tt.HasText("还没有剪贴板历史") || !tt.HasText("正在监听本机剪贴板") {
				t.Fatalf("%s: fresh empty copy missing: %v", theme, tt.Texts())
			}
			if tt.HasText("未选择历史") || tt.HasText("没有匹配的历史") {
				t.Fatalf("%s: legacy split/mismatch empty text leaked: %v", theme, tt.Texts())
			}
			if send, ok := tt.AccessByRole(ui.RoleMenuButton, "发送到设备"); !ok || !send.Disabled {
				t.Fatalf("%s: send toolbar trigger should be disabled when empty: ok=%v disabled=%v", theme, ok, send.Disabled)
			}
			if pin, ok := tt.AccessByRole(ui.RoleButton, "切换收藏"); !ok || !pin.Disabled {
				t.Fatalf("%s: pin toolbar button should be disabled when empty: ok=%v disabled=%v", theme, ok, pin.Disabled)
			}
			saveEmptyShot(t, tt, "01-fresh-empty-"+theme)

			// Clicking "复制示例试试" creates an entry and transitions smoothly out of empty state.
			if err := tt.Click("复制示例试试"); err != nil {
				t.Fatalf("%s: click sample copy: %v", theme, err)
			}
			if len(d.entries) != 1 {
				t.Fatalf("%s: expected 1 sample entry, got %d", theme, len(d.entries))
			}
			if tt.HasText("还没有剪贴板历史") {
				t.Fatalf("%s: empty state did not clear after sample copy", theme)
			}
			if send, ok := tt.AccessByRole(ui.RoleMenuButton, "发送到设备"); !ok || send.Disabled {
				t.Fatalf("%s: send toolbar trigger should be enabled after capture", theme)
			}
		}
	})

	t.Run("cleared-empty-with-paired-devices", func(t *testing.T) {
		d := newDemo("history", "light")
		d.entries = nil
		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{Dark: false})
		tt.SetScale(2)
		tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})

		if !tt.HasText("跨设备同步已就绪") {
			t.Fatalf("expected paired device readiness row, got: %v", tt.Texts())
		}
		if tt.HasText("从旧版导入…") {
			t.Fatalf("import-v1 button should hide when devices are already paired")
		}
		saveEmptyShot(t, tt, "02-cleared-with-devices-light")
	})

	t.Run("scoped-empty-favorites-tag-device", func(t *testing.T) {
		// Favorites empty
		d := newDemo("history", "light")
		for i := range d.entries {
			d.entries[i].pinned = false
		}
		d.nav.Value = "pinned"
		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{Dark: false})
		tt.SetScale(2)
		tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
		if !tt.HasText("还没有收藏的内容") {
			t.Fatalf("empty favorites heading missing: %v", tt.Texts())
		}
		if tt.HasText("未选择历史") {
			t.Fatalf("detail empty pane should be hidden in empty favorites")
		}
		saveEmptyShot(t, tt, "03-empty-pinned-light")

		// Clicking "返回全部项目 (N)" restores all items.
		var backLabel string
		for _, a := range tt.AccessAll() {
			if a.Role == ui.RoleButton && strings.HasPrefix(a.Name, "返回全部项目") {
				backLabel = a.Name
				break
			}
		}
		if backLabel == "" {
			t.Fatal("back to all items button missing")
		}
		if err := tt.Click(backLabel); err != nil {
			t.Fatal(err)
		}
		if d.nav.Value != "all" {
			t.Fatalf("expected nav=all, got %q", d.nav.Value)
		}

		// Tag empty
		d.tags = append(d.tags, historyTag{id: "custom:empty", name: "归档", color: "purple", sidebar: true})
		d.nav.Value = "tag:custom:empty"
		d.tag = "custom:empty"
		tt.SetDark(false)
		if !tt.HasText("“归档” 标签下还没有内容") {
			t.Fatalf("empty tag heading missing: %v", tt.Texts())
		}
		saveEmptyShot(t, tt, "04-empty-tag-light")

		// Device empty (offline)
		d.devices = append(d.devices, device{id: "mba-empty", name: "MacBook Air", online: false})
		d.nav.Value = "device:mba-empty"
		d.source = "mba-empty"
		d.tag = ""
		tt.SetDark(false)
		if !tt.HasText("还没有来自 MacBook Air 的历史") {
			t.Fatalf("empty device heading missing: %v", tt.Texts())
		}
		saveEmptyShot(t, tt, "05-empty-device-light")
	})

	t.Run("search-composite-relaxation", func(t *testing.T) {
		for _, theme := range []string{"light", "dark"} {
			d := newDemo("history", theme)
			d.query = "Notion"
			d.kind = "图片"
			d.timeRange = "today"
			d.search.buffer = d.query
			d.search.last = d.query
			d.search.initialized = true

			tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{Dark: theme == "dark"})
			tt.SetScale(2)
			tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})

			if !tt.HasText("试试放宽条件") {
				t.Fatalf("%s: relaxation card missing: %v", theme, tt.Texts())
			}
			if !tt.HasText("Notion") {
				t.Fatalf("%s: query highlight missing", theme)
			}
			saveEmptyShot(t, tt, "06-search-composite-"+theme)

			// Click first relaxation row to recover results.
			relax := d.activeRelaxations()
			if len(relax) == 0 {
				t.Fatalf("%s: expected non-empty relaxations", theme)
			}
			if err := tt.Click(relax[0].label); err != nil {
				t.Fatalf("%s: click relaxation %q: %v", theme, relax[0].label, err)
			}
			if len(d.filtered()) == 0 {
				t.Fatalf("%s: clicking relaxation did not recover results", theme)
			}
		}
	})

	t.Run("search-keyword-only-and-escape", func(t *testing.T) {
		d := newDemo("history", "dark")
		d.query = "webpack.config.mjs"
		d.search.buffer = d.query
		d.search.last = d.query
		d.search.initialized = true

		tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{Dark: true})
		tt.SetScale(2)
		tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})

		if !tt.HasText("webpack.config.mjs") || !tt.HasText("按条件筛选：") {
			t.Fatalf("keyword-only empty search missing elements: %v", tt.Texts())
		}
		saveEmptyShot(t, tt, "07-search-keyword-dark")

		// Pressing Escape clears the search and restores items.
		tt.Key(0, ui.KeyEscape)
		if d.query != "" || len(d.filtered()) == 0 {
			t.Fatalf("Escape did not clear empty search: query=%q items=%d", d.query, len(d.filtered()))
		}
	})

	t.Run("populated-toolbar-visual", func(t *testing.T) {
		for _, theme := range []string{"light", "dark"} {
			d := newDemo("history", theme)
			tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{Dark: theme == "dark"})
			tt.SetScale(2)
			tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
			saveEmptyShot(t, tt, "08-populated-toolbar-"+theme)
		}
	})
}

func TestHistoryPaneOmitsWarningBanner(t *testing.T) {
	d := newDemo("history", "light")
	l := newStaticLiveState()
	l.loaded = true
	l.key = d.liveQueryKey()
	l.displayedKey = l.key
	l.message = "操作未能确认完成。请刷新数据后检查结果。"
	l.readError = "无法完成搜索。请稍后重试。"
	d.live = l

	tt := ui.NewTesterWith(d.view, 1120, 688, ui.TesterOptions{Dark: false})
	if tt.HasText(l.message) {
		t.Fatalf("unexpected warning banner for live.message in history pane: %v", tt.Texts())
	}
	if tt.HasText(l.readError) {
		t.Fatalf("unexpected warning banner for live.readError in history pane: %v", tt.Texts())
	}
}
