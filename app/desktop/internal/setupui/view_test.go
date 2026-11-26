package setupui

import (
	"runtime"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/internal/v1migration"
)

func TestWelcomeShowcaseScenes(t *testing.T) {
	fields := Fields{}
	begun := 0
	view := ui.NewTesterWith(func(c *ui.Context) {
		Render(c, Model{Stage: "welcome", Found: true}, &fields, Actions{Begin: func() { begun++ }})
	}, 780, 540, ui.TesterOptions{})

	if _, ok := view.AccessByRole(ui.RoleText, "欢迎使用 Unic"); !ok {
		t.Fatal("welcome heading missing")
	}
	if _, ok := view.AccessByRole(ui.RoleText, "VS Code · 刚刚复制"); !ok {
		t.Fatal("default pipeline scene missing")
	}
	if err := view.Click("插件与动作"); err != nil {
		t.Fatalf("switch to actions scene: %v", err)
	}
	view.Frame()
	if _, ok := view.AccessByRole(ui.RoleText, "临时分享"); !ok {
		t.Fatal("actions scene temporary share missing")
	}
	if err := view.Click("跨端直连"); err != nil {
		t.Fatalf("switch to sync scene: %v", err)
	}
	view.Frame()
	if _, ok := view.AccessByRole(ui.RoleText, "Tailcat 端到端加密 · 免云账号"); !ok {
		t.Fatal("sync scene tailcat badge missing")
	}
	if err := view.Click("开始使用"); err != nil {
		t.Fatalf("click begin: %v", err)
	}
	if begun != 1 {
		t.Fatalf("begin action count = %d, want 1", begun)
	}
}

func TestMigrationScopeAndResponsiveLayouts(t *testing.T) {
	for _, size := range [][2]int{{780, 540}, {640, 560}} {
		for _, dark := range []bool{false, true} {
			fields := Fields{Passphrase: "secret"}
			started, unlocked, picked := 0, 0, 0
			actions := Actions{
				Begin:    func() {},
				Redetect: func() {},
				Pick:     func() { picked++ },
				Start:    func() { started++ },
				Unlock:   func() { unlocked++ },
				Stop:     func() {},
				Enter:    func() {},
			}

			confirm := ui.NewTesterWith(func(c *ui.Context) {
				Render(c, Model{Stage: "confirm", Source: "/synthetic/v1", Found: true}, &fields, actions)
			}, size[0], size[1], ui.TesterOptions{Dark: dark})
			for _, label := range []string{"收藏", "兼容的设置", "设备身份", "其他设备需升级到 v2 后重新配对"} {
				if _, ok := confirm.AccessByRole(ui.RoleText, label); !ok {
					t.Fatalf("size=%v dark=%v missing confirm scope text %q", size, dark, label)
				}
			}
			if err := confirm.Click("开始迁移"); err != nil {
				t.Fatalf("click start at %v: %v", size, err)
			}
			if started != 1 {
				t.Fatalf("start count = %d, want 1", started)
			}

			pass := ui.NewTesterWith(func(c *ui.Context) {
				Render(c, Model{Stage: "password", Source: "/synthetic/v1", Error: "无法解锁旧数据。请核对 v1 口令后重试。"}, &fields, actions)
			}, size[0], size[1], ui.TesterOptions{Dark: dark})
			if _, ok := pass.AccessByRole(ui.RoleAlert, "无法解锁旧数据。请核对 v1 口令后重试。"); !ok {
				t.Fatalf("size=%v dark=%v missing passphrase alert", size, dark)
			}
			if err := pass.Click("重试系统凭据"); err != nil {
				t.Fatalf("click retry credentials: %v", err)
			}
			if started != 2 {
				t.Fatalf("retry credentials did not call Start")
			}
			if err := pass.Click("解锁并继续"); err != nil {
				t.Fatalf("click unlock: %v", err)
			}
			if unlocked != 1 {
				t.Fatalf("unlock count = %d, want 1", unlocked)
			}

			failed := ui.NewTesterWith(func(c *ui.Context) {
				Render(c, Model{Stage: "failed", Source: "/synthetic/v1", Report: v1migration.Report{Status: "failed", Imported: 2}}, &fields, actions)
			}, size[0], size[1], ui.TesterOptions{Dark: dark})
			if err := failed.Click("选择其他目录"); err != nil {
				t.Fatalf("click pick from failed: %v", err)
			}
			if picked != 1 {
				t.Fatalf("pick count = %d, want 1", picked)
			}
		}
	}
}

func TestMacIntegratedCaptionClearance(t *testing.T) {
	if runtime.GOOS != "darwin" {
		t.Skip("macOS-integrated traffic-light band applies on darwin only")
	}
	for _, size := range [][2]int{{780, 540}, {640, 560}} {
		fields := Fields{}
		confirm := ui.NewTesterWith(func(c *ui.Context) {
			Render(c, Model{Stage: "confirm", Source: "/synthetic/v1", Found: true}, &fields, Actions{Start: func() {}, Enter: func() {}, Pick: func() {}})
		}, size[0], size[1], ui.TesterOptions{})
		confirm.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
		confirm.Frame()

		checkLabel := "从 v1 迁移历史"
		if size[0] < int(compactBreakpoint) {
			checkLabel = "确认目录与范围"
		}
		node, ok := confirm.AccessByRole(ui.RoleText, checkLabel)
		if !ok {
			t.Fatalf("size=%v missing %q", size, checkLabel)
		}
		if node.Bounds.Y < 42 {
			t.Fatalf("size=%v %q overlaps macOS traffic-light band: %+v", size, checkLabel, node.Bounds)
		}
	}
}
