package main

import (
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func eachGUISkin(t *testing.T, run func(*testing.T, bool)) {
	t.Helper()
	for _, skin := range []string{"linux", "macos", "windows"} {
		t.Run(skin, func(t *testing.T) {
			t.Setenv("UNIC_UI_SKIN", skin)
			for _, dark := range []bool{false, true} {
				name := "light"
				if dark {
					name = "dark"
				}
				t.Run(name, func(t *testing.T) { run(t, dark) })
			}
		})
	}
}

func fixtureGUI() *gui {
	return &gui{
		daemonUp: true, historyLoaded: true,
		status: api.Status{
			ID: "preview-device", Name: "我的电脑",
			Peers: []api.PeerStatus{{ID: "other-device", Name: "另一台电脑", Connected: true}},
		},
		hasCode: true,
		code:    api.Code{Code: "123-456", ExpiresAtMs: time.Now().Add(4 * time.Minute).UnixMilli()},
		history: []api.HistoryItem{{
			ID: "history-one", Kind: "text", Origin: "migration",
			CreatedAt: 1794182400000, Tags: []string{"工作", "备忘"},
		}},
		historyPreviews: map[string]string{"history-one": "跨设备剪贴板 · 示例历史内容"},
	}
}

func guiClick(t *testing.T, tt *ui.Tester, label string) {
	t.Helper()
	if err := tt.Click(label); err != nil {
		t.Fatal(err)
	}
}

func guiScreenshot(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS")
	if dir == "" {
		return
	}
	dir = filepath.Join(dir, t.Name())
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

// Exercise the actual window view so skin changes cannot lose navigation, approval
// information, history tags or the destructive confirmation barrier.
func scenarioGUINavigation(t *testing.T) {
	eachGUISkin(t, func(t *testing.T, dark bool) {
		g := fixtureGUI()
		tt := ui.NewTester(g.view, 480, 640)
		proof.Capture(t, tt)
		tt.SetDark(dark)
		if !tt.HasText("跨设备剪贴板") || !tt.HasText("工作") {
			t.Fatalf("history or tags missing: %v", tt.Texts())
		}
		guiScreenshot(t, tt, "history")
		guiClick(t, tt, "设备")
		if g.tab != tabDevices || !tt.HasText("另一台电脑") {
			t.Fatal("devices tab did not open")
		}
		guiScreenshot(t, tt, "devices")
		guiClick(t, tt, "移除…")
		if g.confirmRemove == "" || !tt.HasText("之后不再同步") {
			t.Fatal("remove skipped its confirmation")
		}
		guiScreenshot(t, tt, "remove")
		tt.Key(0, ui.KeyEnter) // Destructive alert initially focuses Cancel.
		if g.confirmRemove != "" {
			t.Fatal("Return did not choose the safe initial action")
		}
		guiClick(t, tt, "移除…")
		tt.Key(0, ui.KeyEscape)
		if g.confirmRemove != "" {
			t.Fatal("Escape did not dismiss the confirmation")
		}
		guiClick(t, tt, "动态")
		if !tt.HasText("暂无动态") {
			t.Fatal("activity tab did not open")
		}
		guiClick(t, tt, "添加设备")
		if !g.showing || !tt.HasText("123 456") {
			t.Fatal("pairing code pane did not open")
		}
		guiScreenshot(t, tt, "pairing")
		guiClick(t, tt, "加入其他设备")
		if g.showing {
			t.Fatal("hidden pairing code remained marked visible")
		}
		guiScreenshot(t, tt, "join")
		code, ok := tt.AccessByRole(ui.RoleTextField, "6 位配对码")
		if !ok {
			t.Fatal("code input missing")
		}
		tt.ClickAt(code.Bounds.X+code.Bounds.W/2, code.Bounds.Y+code.Bounds.H/2)
		tt.Type("１２３")
		tt.Key(0, ui.KeyEnter) // An incomplete code must never start a request.
		if g.joinBusy || g.codeInput != "123" {
			t.Fatal("incomplete or full-width input handling changed")
		}
		if a, ok := tt.Access("配对"); !ok || !a.Disabled {
			t.Fatal("incomplete code can be submitted")
		}
		tt.Type("４５６")
		if a, ok := tt.Access("配对"); !ok || a.Disabled {
			t.Fatal("complete code cannot be submitted")
		}
		guiClick(t, tt, "完整地址")
		if _, ok := tt.AccessByRole(ui.RoleTextField, "完整地址"); !ok {
			t.Fatal("address input missing")
		}
		guiScreenshot(t, tt, "address")
	})
}

func scenarioGUIApprovalAndReconnect(t *testing.T) {
	eachGUISkin(t, func(t *testing.T, dark bool) {
		g := fixtureGUI()
		g.tab = tabDevices
		g.status.Pending = []api.PendingPeer{{
			ID: "pending-device", Name: "新电脑", ByCode: true,
			Fingerprint: "ABCD EFGH IJKL", Time: time.Now().Unix(),
		}}
		tt := ui.NewTester(g.view, 480, 640)
		proof.Capture(t, tt)
		tt.SetDark(dark)
		if !tt.HasText("ABCD EFGH IJKL") || !tt.HasText("指纹一致,批准") || !tt.HasText("拒绝") {
			t.Fatal("code pairing lost its explicit fingerprint approval")
		}
		guiScreenshot(t, tt, "approval")
		g.daemonUp = false
		tt.Frame()
		if !tt.HasText("后台节点未运行") || g.showing {
			t.Fatal("disconnected state is incorrect")
		}
		guiScreenshot(t, tt, "disconnected")
		g.daemonUp = true
		tt.Frame()
		if !tt.HasText("新电脑") {
			t.Fatal("view did not recover after reconnect")
		}
	})
}

func scenarioGUIEmptyHistoryLayout(t *testing.T) {
	eachGUISkin(t, func(t *testing.T, dark bool) {
		g := fixtureGUI()
		g.history = nil
		tt := ui.NewTester(g.view, 480, 640)
		proof.Capture(t, tt)
		tt.SetDark(dark)
		tt.SetScale(2)
		guiClick(t, tt, "按标签筛选")
		guiScreenshot(t, tt, "empty-focused")

		empty, ok := tt.AccessByRole(ui.RoleStatus, "暂无历史")
		if !ok || empty.Bounds.H < 300 {
			t.Fatalf("empty state did not receive the remaining body height: %+v", empty.Bounds)
		}
		last, ok := tt.AccessByRole(ui.RoleRadio, "添加设备")
		if !ok || last.Bounds.X+last.Bounds.W < 450 {
			t.Fatalf("tabs leave an unused track at the trailing edge: %+v", last.Bounds)
		}
		if os.Getenv("UNIC_UI_SKIN") == "macos" {
			selected, ok := tt.AccessByRole(ui.RoleRadio, "历史")
			if !ok || selected.Bounds.H != 20 {
				t.Fatalf("selected segment does not fill the 24pt track: %+v", selected.Bounds)
			}
			field, ok := tt.AccessByRole(ui.RoleTextField, "按标签筛选")
			if !ok || field.Bounds.H != 22 {
				t.Fatalf("text field does not use the 22pt macOS metric: %+v", field.Bounds)
			}
			r := field.Bounds
			img := tt.Image()
			sample := func(x, y float32) color.RGBA {
				return img.RGBAAt(int(x*2), int(y*2))
			}
			left := sample(r.X-2, r.Y+r.H/2)
			right := sample(r.X+r.W+2, r.Y+r.H/2)
			top := sample(r.X+r.W/2, r.Y-2)
			if left != right || left != top || left.B <= left.R {
				t.Fatalf("focus ring clipped or asymmetric: left=%v right=%v top=%v", left, right, top)
			}
		}
		tt.SetSize(360, 520)
		for _, label := range []string{"按标签筛选", "筛选", "最新"} {
			a, ok := tt.Access(label)
			if !ok || a.Bounds.X < 0 || a.Bounds.X+a.Bounds.W > 360 {
				t.Fatalf("%s overflows the narrow window: %+v", label, a.Bounds)
			}
		}
		guiScreenshot(t, tt, "empty-narrow")
	})
}
