package desktopdemo

import (
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
)

// Failure cases: sending immediately without a choice; a custom painted menu
// on macOS; stale device names; local device appearing as a send target.
func TestSendDeviceMenuE2E(t *testing.T) {
	type result struct {
		Theme, Choice string
		Items         []string
		Notice        string
	}
	var results []result
	defer func() {
		if dir := os.Getenv("UNIC_UI_TEST_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			b, err := json.MarshalIndent(results, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err := os.WriteFile(filepath.Join(dir, "send-device-menu.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, theme := range []string{"dark", "light"} {
		for _, choice := range []string{"所有设备", "工作电脑", "Linux 工作站", "我的手机", "cancel", "updated"} {
			t.Run(theme+"/"+choice, func(t *testing.T) {
				t.Setenv("UNIC_UI_SKIN", "macos")
				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				if choice == "updated" {
					d.devices[1].name = "办公室电脑"
					d.devices = slices.DeleteFunc(d.devices, func(v device) bool { return v.id == "phone" })
					tt.Frame()
				}
				trigger, ok := tt.AccessByRole(ui.RoleMenuButton, "发送到…")
				if !ok {
					t.Fatal("detail send menu trigger missing")
				}
				tt.Press(trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2)
				items := tt.Menu()
				if !slices.Contains(items, "所有设备") {
					t.Fatalf("native menu missing all devices: %v", items)
				}
				for _, v := range d.devices {
					if v.id != "local" && !slices.Contains(items, v.name) {
						t.Fatalf("missing target %s: %v", v.name, items)
					}
					if v.id == "local" && slices.Contains(items, v.name) {
						t.Fatal("local device offered")
					}
				}
				if _, ok := tt.AccessByRole(ui.RoleMenu, ""); ok {
					t.Fatal("menu painted in window instead of native backend")
				}
				if d.notice != "" {
					t.Fatal("opening menu performed send")
				}
				if choice == "cancel" {
					tt.CloseMenu()
					if d.notice != "" {
						t.Fatal("cancelling performed send")
					}
				} else {
					target := choice
					if choice == "updated" {
						target = "办公室电脑"
						if slices.Contains(items, "工作电脑") || slices.Contains(items, "我的手机") {
							t.Fatal("stale targets")
						}
					}
					if err := tt.ChooseMenuItem(target); err != nil {
						t.Fatal(err)
					}
					if d.notice != "演示：已发送到"+target {
						t.Fatalf("wrong send target: %s", d.notice)
					}
				}
				results = append(results, result{theme, choice, items, d.notice})
			})
		}
	}
}
