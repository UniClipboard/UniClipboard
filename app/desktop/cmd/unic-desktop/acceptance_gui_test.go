package main

import (
	"image/png"
	"math"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func eachGUITheme(t *testing.T, run func(*testing.T, bool)) {
	t.Helper()
	for _, skin := range []string{"boardui"} {
		t.Run(skin, func(t *testing.T) {
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
	eachGUITheme(t, func(t *testing.T, dark bool) {
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
	eachGUITheme(t, func(t *testing.T, dark bool) {
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
	eachGUITheme(t, func(t *testing.T, dark bool) {
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
		last, ok := tt.AccessByRole(ui.RoleToggleButton, "添加设备")
		if !ok || last.Bounds.W <= 0 || last.Bounds.X+last.Bounds.W > 480 {
			t.Fatalf("source toggle navigation is missing or leaves the viewport: %+v", last.Bounds)
		}
		field, ok := tt.AccessByRole(ui.RoleTextField, "按标签筛选")
		if !ok || field.Bounds.H != boardui.Geometry("input", "medium").Height || !field.Focused {
			t.Fatalf("source field geometry/focus: %+v", field)
		}
		r := field.Bounds
		img := tt.Image()
		edge := boardui.Token(dark, "border-button-active")
		for _, point := range [][2]float32{{r.X + 1, r.Y + r.H/2}, {r.X + r.W - 1, r.Y + r.H/2}, {r.X + r.W/2, r.Y + 1}} {
			ink := img.RGBAAt(int(point[0]*2), int(point[1]*2))
			if math.Abs(float64(ink.R)-float64(edge.R)) > 2 || math.Abs(float64(ink.G)-float64(edge.G)) > 2 || math.Abs(float64(ink.B)-float64(edge.B)) > 2 {
				t.Fatalf("source focus inset clipped at %v: got %v want %v", point, ink, edge)
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
