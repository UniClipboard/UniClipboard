package e2e_test

import (
	"fmt"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// This complete gallery uses the public component API. Check every pinned
// Button variant/size through real pointer and key input before adding controls.
func scenarioStatesEndToEnd(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, scale := range []float32{1, 2} {
			name := fmt.Sprintf("states-%t-%gx", dark, scale)
			t.Run(name, func(t *testing.T) {
				if err := boardui.LoadFonts(); err != nil {
					t.Fatal(err)
				}
				k := boardui.Kit{Dark: dark}
				count := 0
				text := "中文 日本語 한국어 🙂"
				role := "member"
				open := false
				view := func(c *ui.Context) {
					k.Apply(c)
					ui.Column(c).Fill().Padding(24).Gap(8).Background(boardui.Token(dark, "background-full")).Children(func() {
						for _, size := range []string{"medium", "small", "xs"} {
							for _, variant := range []string{"primary", "secondary", "ghost", "danger"} {
								id := size + "-" + variant
								c.Key(id)
								ui.Row(c).Gap(12).AlignItems(ui.Center).Children(func() {
									if k.Button(c, id, boardui.ButtonProps{Size: size, Variant: variant}).Clicked() {
										count++
									}
									k.Button(c, "", boardui.ButtonProps{Size: size, Variant: variant, Icon: "settings", Label: "Add " + id})
									k.Button(c, "Disabled "+id, boardui.ButtonProps{Size: size, Variant: variant, Disabled: true})
								})
							}
						}
						k.Input(c, &text, boardui.InputProps{Label: "Gallery input", Description: "Mixed-script text", Size: "small"})
						k.Select(c, boardui.SelectProps{Label: "Gallery select", Value: &role, Open: &open, Size: "sm", Items: []boardui.SelectItem{{Value: "member", Label: "Member"}, {Value: "blocked", Label: "Blocked", Disabled: true}, {Value: "admin", Label: "Admin"}}})
					})
				}
				tt := ui.NewTesterWith(view, 1000, 760, ui.TesterOptions{Dark: dark})
				tt.SetLocale("en-US")
				tt.SetScale(scale)
				capture(t, tt, name+"-default")
				for _, size := range []string{"medium", "small", "xs"} {
					for _, variant := range []string{"primary", "secondary", "ghost", "danger"} {
						id := size + "-" + variant
						a := mustAccess(t, tt, ui.RoleButton, id)
						expected := map[string]float32{"medium": 36, "small": 32, "xs": 24}[size]
						if a.Bounds.H != expected {
							t.Fatalf("%s height=%v want %v", id, a.Bounds.H, expected)
						}
						if !mustAccess(t, tt, ui.RoleButton, "Disabled "+id).Disabled {
							t.Fatalf("disabled %s exposed enabled", id)
						}
						mustAccess(t, tt, ui.RoleButton, "Add "+id)
						x, y := a.Bounds.X+a.Bounds.W/2, a.Bounds.Y+a.Bounds.H/2
						tt.Move(x, y)
						tt.Advance(200 * time.Millisecond)
						capture(t, tt, name+"-"+id+"-hover")
						tt.Press(x, y)
						tt.Advance(220 * time.Millisecond)
						capture(t, tt, name+"-"+id+"-pressed")
						tt.Release(x, y)
						tt.Advance(420 * time.Millisecond)
					}
				}
				if count != 12 {
					t.Fatalf("button activation count=%d", count)
				}
				// Keyboard focus, disabled skip and selection are full view interactions.
				mustClick(t, tt, "Gallery select")
				tt.Key(0, ui.KeyEnd)
				tt.Key(0, ui.KeyEnter)
				if role != "admin" {
					t.Fatalf("disabled skip/End selection=%s", role)
				}
				tt.Key(0, ui.KeyTab)
				capture(t, tt, name+"-focus")
				tt.Advance(time.Second)
				capture(t, tt, name+"-settled")
			})
		}
	}
	scenarioNarrowReducedMotionEndToEnd(t)
}
