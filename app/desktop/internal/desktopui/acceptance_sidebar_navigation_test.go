package desktopui

import (
	"encoding/json"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure inventory: hovered and selected rows paint identically; a hover
// mutates the filter; two rows expose selection; a scrollbar covers row counts;
// overflow pushes the fixed footer out; tag/all selection stops filtering.
// Drive the complete history fixture, including its actual sidebar and scroll.
func scenarioSidebarNavigation(t *testing.T) {
	for _, theme := range []string{"light", "dark"} {
		for _, scale := range []float32{1, 2} {
			t.Run(fmt.Sprintf("%s/scale%.0f", theme, scale), func(t *testing.T) {
				d := newDemo("history", theme)
				for i := 0; i < 24; i++ {
					d.devices = append(d.devices, device{id: fmt.Sprintf("overflow-%d", i), name: fmt.Sprintf("Sidebar overflow device %d", i)})
				}
				tt := ui.NewTesterWith(d.view, 1100, 900, ui.TesterOptions{Dark: theme == "dark"})
				tt.SetScale(scale)
				tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
				proof.Capture(t, tt)
				row := func(label string) ui.Rect {
					t.Helper()
					a, ok := tt.AccessByRole(ui.RoleListItem, label)
					if !ok {
						t.Fatalf("sidebar row missing: %s", label)
					}
					return a.Bounds
				}
				all, work := row("全部项目"), row("工作")
				// Overall layout: symmetric 12-DIP outer inset, compact 30-DIP rows,
				// quiet inline section actions, grouped spacing and aligned More text.
				if all.X != 12 || all.H != 30 || work.X != all.X || work.H != all.H || all.X+all.W != 238 {
					t.Errorf("sidebar has asymmetric padding or inconsistent row density: all=%+v tag=%+v", all, work)
				}
				if _, ok := tt.AccessByRole(ui.RoleListItem, "文本"); ok {
					t.Error("builtin content types should stay out of the sidebar by default")
				}
				toggle, ok := tt.AccessByRole(ui.RoleButton, "切换侧栏")
				if !ok || toggle.Bounds.W != 28 || toggle.Bounds.H != 28 {
					t.Errorf("sidebar toggle must retain its 28-DIP hit target: %+v", toggle.Bounds)
				}
				if runtime.GOOS == "darwin" {
					if toggle.Bounds.X+toggle.Bounds.W != 238 {
						t.Errorf("macOS toggle is misaligned with navigation right edge: %+v", toggle.Bounds)
					}
				} else {
					bar, found := tt.AccessByRole(ui.RoleToolbar, "全部")
					if !found || toggle.Bounds.X != bar.Bounds.X+16 || toggle.Bounds.Y < bar.Bounds.Y || toggle.Bounds.Y+toggle.Bounds.H > bar.Bounds.Y+bar.Bounds.H {
						t.Errorf("host toolbar toggle is outside its leading inset: toggle=%+v toolbar=%+v", toggle.Bounds, bar.Bounds)
					}
				}
				action, ok := tt.AccessByRole(ui.RoleButton, "管理标签")
				if !ok || action.Bounds.W != 24 || action.Bounds.H != 24 {
					t.Errorf("section action is oversized: %+v", action.Bounds)
				}
				pinned := row("已置顶")
				if action.Bounds.Y < pinned.Y+pinned.H+12 {
					t.Errorf("sidebar sections run together: pinned=%+v action=%+v", pinned, action.Bounds)
				}
				var tagLabel ui.Rect
				for _, a := range tt.AccessAll() {
					if a.Role != ui.RoleText || a.Bounds.X >= 250 {
						continue
					}
					if a.Name == "工作" {
						tagLabel = a.Bounds
					}
				}
				more, ok := tt.AccessByRole(ui.RoleButton, "全部标签 · 10")
				if !ok || more.Bounds.Y+more.Bounds.H > 900 {
					t.Fatal("More must be visible for text alignment capture")
				}
				// Native buttons flatten their text into the accessible name. Measure
				// the actual rendered ink instead of requiring a duplicate AX text node.
				pixels := tt.Image()
				bg := pixels.RGBAAt(int((more.Bounds.X+2)*scale), int((more.Bounds.Y+2)*scale))
				inkLeft := more.Bounds.X + more.Bounds.W
				for x := int(more.Bounds.X * scale); x < int((more.Bounds.X+more.Bounds.W)*scale); x++ {
					found := false
					for y := int((more.Bounds.Y + 2) * scale); y < int((more.Bounds.Y+more.Bounds.H-2)*scale); y++ {
						p := pixels.RGBAAt(x, y)
						if absBoardUIChannel(int(p.R)-int(bg.R))+absBoardUIChannel(int(p.G)-int(bg.G))+absBoardUIChannel(int(p.B)-int(bg.B)) > 60 {
							found = true
							break
						}
					}
					if found {
						inkLeft = float32(x) / scale
						break
					}
				}
				if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
					name := fmt.Sprintf("sidebar-layout-%s-%.0fx", theme, scale)
					f, err := os.Create(filepath.Join(dir, name+".png"))
					if err != nil {
						t.Fatal(err)
					}
					if err := png.Encode(f, pixels); err != nil {
						t.Fatal(err)
					}
					if err := f.Close(); err != nil {
						t.Fatal(err)
					}
					data, err := json.MarshalIndent(map[string]any{"all": all, "tag": work, "section_action": action.Bounds, "more": more.Bounds, "tag_label": tagLabel, "more_first_ink": inkLeft}, "", "  ")
					if err != nil {
						t.Fatal(err)
					}
					if err := os.WriteFile(filepath.Join(dir, name+".json"), data, 0644); err != nil {
						t.Fatal(err)
					}
				}
				if tagLabel.W == 0 || inkLeft < tagLabel.X-3 || inkLeft > tagLabel.X+3 {
					t.Errorf("More is not aligned with navigation labels: tag=%+v firstInk=%v", tagLabel, inkLeft)
				}
				tt.SetSize(1100, 480)
				tt.Move(work.X+work.W/2, work.Y+work.H/2)
				tt.Frame()
				if d.nav.Value != "all" || d.tag != "" {
					t.Fatal("hover changed history filter")
				}
				image := tt.Image()
				selected := image.RGBAAt(int((all.X+3)*scale), int((all.Y+all.H/2)*scale))
				hovered := image.RGBAAt(int((work.X+3)*scale), int((work.Y+work.H/2)*scale))
				if selected == hovered {
					t.Errorf("hover and selected navigation paint are identical: %v", selected)
				}
				var scroll ui.Rect
				for _, a := range tt.AccessAll() {
					if a.Role == ui.RoleScroll && a.Bounds.X < 250 && a.Bounds.W > 100 {
						scroll = a.Bounds
						break
					}
				}
				if scroll.W == 0 {
					t.Fatal("sidebar scroll viewport missing")
				}
				if all.X+all.W > scroll.X+scroll.W-12 {
					t.Errorf("sidebar row overlaps scrollbar gutter: row=%+v scroll=%+v", all, scroll)
				}
				footer := row("设置…")
				if footer.Y < 0 || footer.Y+footer.H > 480 {
					t.Fatalf("fixed footer outside window: %+v", footer)
				}
				tt.ClickAt(work.X+work.W/2, work.Y+work.H/2)
				tt.Frame()
				if d.nav.Value != "tag:工作" || d.tag != "工作" || len(d.filtered()) != 3 {
					t.Fatalf("tag selection failed: nav=%s tag=%s results=%d", d.nav.Value, d.tag, len(d.filtered()))
				}
				a, _ := tt.AccessByRole(ui.RoleListItem, "全部项目")
				b, _ := tt.AccessByRole(ui.RoleListItem, "工作")
				if a.Selected || !b.Selected {
					t.Fatalf("sidebar is not single selection: all=%v work=%v", a.Selected, b.Selected)
				}
				all = row("全部项目")
				tt.ClickAt(all.X+all.W/2, all.Y+all.H/2)
				tt.Frame()
				if d.nav.Value != "all" || d.tag != "" || len(d.filtered()) != 22 {
					t.Fatal("all-items navigation did not clear the tag filter")
				}
				tt.Scroll(scroll.X+scroll.W-18, scroll.Y+scroll.H/2, 0, 120)
				tt.Advance(16 * time.Millisecond)
				footer = row("设置…")
				if footer.Y+footer.H > 480 {
					t.Fatal("scrolling displaced the fixed footer")
				}
				tt.Advance(800 * time.Millisecond)
			})
		}
	}
}
