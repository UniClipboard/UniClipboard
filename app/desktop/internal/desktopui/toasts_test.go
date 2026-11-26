package desktopui

import (
	"context"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestDesktopToastOverlayDoesNotShiftLayoutAndDismisses(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	d := newDemo("history", "light")
	d.live = &liveState{
		ctx:         ctx,
		cancel:      cancel,
		client:      api.NewClient(filepath.Join(t.TempDir(), "unused.sock")),
		loaded:      true,
		loading:     true,
		nextRefresh: time.Now().Add(time.Hour),
		results:     make(chan func(), 32),
		jobs:        make(chan func(context.Context) func(), 16),
		items:       map[string]api.HistoryItem{},
		details:     map[string]entry{},
		pending:     map[string]api.PendingPeer{},
		config:      config.State{Revision: "rev-1", Config: config.Default()},
	}
	screen := &Desktop{d: d}

	tt := ui.NewTesterWith(screen.View, 1100, 720, ui.TesterOptions{})
	before, ok := tt.Find("全部")
	if !ok {
		t.Fatalf("missing history sidebar anchor before toast: %v", tt.Texts())
	}

	const migErr = "无法停止当前后台服务。请退出独立服务后重试迁移。"
	screen.ShowErrorToast(migErr)
	tt.Frame()

	after, ok := tt.Find("全部")
	if !ok {
		t.Fatal("missing history sidebar anchor while toast is visible")
	}
	if before != after {
		t.Fatalf("floating error toast shifted workspace layout: before=%+v after=%+v", before, after)
	}
	if !tt.HasText(migErr) {
		t.Fatalf("error toast text missing: %v", tt.Texts())
	}
	if _, ok := tt.AccessByRole(ui.RoleAlert, ""); !ok {
		t.Fatal("error toast did not expose RoleAlert")
	}

	// Clicking the BoardUI close button dismisses the toast.
	if err := tt.Click("关闭提示"); err != nil {
		t.Fatalf("click toast close button: %v", err)
	}
	tt.Frame()
	if tt.HasText(migErr) {
		t.Fatal("error toast remained after close button click")
	}

	// Persistent restart warning respects manual dismissal until condition changes.
	d.live.config.RestartRequired = []string{"plugins"}
	tt.Frame()
	if !tt.HasText("插件设置已保存。请退出并重新启动桌面端以生效。") {
		t.Fatalf("restart toast missing: %v", tt.Texts())
	}
	if err := tt.Click("关闭提示"); err != nil {
		t.Fatalf("dismiss restart toast: %v", err)
	}
	tt.Frame()
	if tt.HasText("插件设置已保存。请退出并重新启动桌面端以生效。") {
		t.Fatal("dismissed restart toast reappeared for the same condition")
	}

	// Clearing and re-triggering shows the toast again.
	d.live.config.RestartRequired = nil
	tt.Frame()
	d.live.config.RestartRequired = []string{"plugins"}
	tt.Frame()
	if !tt.HasText("插件设置已保存。请退出并重新启动桌面端以生效。") {
		t.Fatal("restart toast did not reappear after condition reset")
	}

	// Move pointer away from the bottom-right toast stack so hover pause does not hold the timer.
	tt.Move(10, 10)
	// Timed toast auto-dismisses cleanly.
	screen.ShowToast(desktopkit.ToastSuccess, "配置备份已导出", desktopkit.ToastOptions{Duration: time.Second})
	tt.Frame()
	if !tt.HasText("配置备份已导出") {
		t.Fatal("success toast missing")
	}
	tt.Advance(1300 * time.Millisecond)
	if d.toasts.Len() != 1 || tt.HasText("配置备份已导出") {
		t.Fatalf("timed toast did not auto-dismiss (toasts=%d)", d.toasts.Len())
	}

	if snapDir := os.Getenv("UNIC_TOAST_SNAP_DIR"); snapDir != "" {
		_ = os.MkdirAll(snapDir, 0755)
		for _, theme := range []string{"light", "dark"} {
			sd := newDemo("history", theme)
			sd.live = &liveState{
				ctx:         ctx,
				cancel:      cancel,
				client:      api.NewClient(filepath.Join(t.TempDir(), "unused.sock")),
				loaded:      true,
				loading:     true,
				nextRefresh: time.Now().Add(time.Hour),
				results:     make(chan func(), 32),
				jobs:        make(chan func(context.Context) func(), 16),
				items:       map[string]api.HistoryItem{},
				details:     map[string]entry{},
				pending:     map[string]api.PendingPeer{},
				config:      config.State{Revision: "rev-1", Config: config.Default(), RestartRequired: []string{"plugins"}},
				restartApp:  func(bool, func(error)) {},
			}
			sc := &Desktop{d: sd}
			sc.ShowToast(desktopkit.ToastSuccess, "配置已导出", desktopkit.ToastOptions{
				Description: "偏好设置已保存至本地备份文件。",
				Duration:    5 * time.Second,
			})
			sc.ShowErrorToast(migErr)
			st := ui.NewTesterWith(sc.View, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
			st.SetScale(2)
			st.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
			st.Advance(350 * time.Millisecond)
			if f, err := os.Create(filepath.Join(snapDir, "toast-"+theme+".png")); err == nil {
				_ = png.Encode(f, st.Image())
				_ = f.Close()
			}
		}
	}
}

func TestToastClearOfPluginsSettingsControls(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	d := newDemo("settings", "light")
	d.settings.Value = "plugins"
	d.live = &liveState{
		ctx:               ctx,
		cancel:            cancel,
		client:            api.NewClient(filepath.Join(t.TempDir(), "unused.sock")),
		loaded:            true,
		loading:           true,
		nextRefresh:       time.Now().Add(time.Hour),
		results:           make(chan func(), 32),
		jobs:              make(chan func(context.Context) func(), 16),
		items:             map[string]api.HistoryItem{},
		details:           map[string]entry{},
		pending:           map[string]api.PendingPeer{},
		config:            config.State{Revision: "rev-1", Config: config.Default(), RestartRequired: []string{"plugins"}},
		restartApp:        func(bool, func(error)) {},
		marketplaceLoaded: true,
	}
	screen := &Desktop{d: d}

	rectsOverlap := func(a, b ui.Rect) bool {
		return a.X < b.X+b.W && a.X+a.W > b.X && a.Y < b.Y+b.H && a.Y+a.H > b.Y
	}

	// 1. Plugins list view at Linux CI geometry (1100x720, 1000x640 docked, and compact 860x640 overlay).
	for _, size := range [][2]int{{1100, 720}, {1000, 640}, {860, 640}} {
		tt := ui.NewTesterWith(screen.View, size[0], size[1], ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
		toastRegion, ok := tt.AccessByRole(ui.RoleRegion, "通知")
		if !ok {
			t.Fatalf("size=%v missing toast region", size)
		}
		for _, target := range []struct {
			role  ui.Role
			label string
		}{
			{ui.RoleButton, "文本工具"},
			{ui.RoleSwitch, "启用文本工具"},
			{ui.RoleButton, "内容分类"},
			{ui.RoleSwitch, "启用内容分类"},
		} {
			ctrl, ok := tt.AccessByRole(target.role, target.label)
			if !ok {
				t.Fatalf("size=%v missing %s", size, target.label)
			}
			if rectsOverlap(toastRegion.Bounds, ctrl.Bounds) {
				t.Fatalf("size=%v toast %+v overlaps %q %+v", size, toastRegion.Bounds, target.label, ctrl.Bounds)
			}
		}
	}

	// 2. Plugin detail view: back button, enable switch, and failure policy select trigger must be clear.
	d.pluginSettings.id = "builtin.text"
	tt := ui.NewTesterWith(screen.View, 1100, 720, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	toastRegion, ok := tt.AccessByRole(ui.RoleRegion, "通知")
	if !ok {
		t.Fatal("detail view missing toast region")
	}
	for _, target := range []struct {
		role  ui.Role
		label string
	}{
		{ui.RoleButton, "返回插件列表"},
		{ui.RoleSwitch, "启用文本工具"},
		{ui.RolePopUpButton, "处理失败时"},
	} {
		ctrl, ok := tt.AccessByRole(target.role, target.label)
		if !ok {
			t.Fatalf("detail view missing %s", target.label)
		}
		if rectsOverlap(toastRegion.Bounds, ctrl.Bounds) {
			t.Fatalf("detail view toast %+v overlaps %q %+v", toastRegion.Bounds, target.label, ctrl.Bounds)
		}
	}

	// 3. Open the failure policy Select while the restart toast is visible AND a
	// background history refresh (l.loading) starts mid-interaction; the menu
	// options must remain clear of the toast and choosing "继续处理" must save.
	d.live.loading = false
	d.live.busy = false
	d.live.config.Config.Plugins.Instances = map[string]config.Instance{
		"builtin.text": {OnError: "block_sync"},
	}
	tt = ui.NewTesterWith(screen.View, 1100, 720, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, TextScale: 1}})
	tt.Frame()
	trigger, ok := tt.AccessByRole(ui.RolePopUpButton, "处理失败时")
	if !ok {
		t.Fatal("missing trigger")
	}
	tx, ty := trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2
	tt.Press(tx, ty)
	tt.Release(tx, ty)
	toastRegion, ok = tt.AccessByRole(ui.RoleRegion, "通知")
	if !ok {
		t.Fatal("detail open-menu view missing toast region")
	}
	for _, n := range tt.AccessAll() {
		if n.Role == ui.RoleOption && !n.Offscreen && rectsOverlap(toastRegion.Bounds, n.Bounds) {
			t.Fatalf("toast %+v overlaps open select option %q %+v", toastRegion.Bounds, n.Name, n.Bounds)
		}
	}
	// Simulate a background SSE refresh (e.g. config watcher reload) starting
	// right before the user clicks an option in the already-open Select menu.
	d.live.loading = true
	if err := boarduiChooseMenu(tt, "继续处理"); err != nil {
		t.Fatalf("boarduiChooseMenu: %v", err)
	}
	if !d.live.busy {
		t.Fatalf("clicking 继续处理 during background loading did not trigger saveLiveConfig (toast=%+v trigger=%+v)", toastRegion.Bounds, trigger.Bounds)
	}

	// 4. History page: bottom-center toast must stay clear of top toolbar / search box.
	d.page = "history"
	d.live.busy = false
	tt = ui.NewTesterWith(screen.View, 1100, 720, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, TextScale: 1}})
	tt.Frame()
	toastRegion, ok = tt.AccessByRole(ui.RoleRegion, "通知")
	if !ok {
		t.Fatal("history view missing toast region")
	}
	if search, ok := tt.AccessByRole(ui.RoleSearch, ""); !ok {
		t.Fatal("history view missing search box")
	} else if rectsOverlap(toastRegion.Bounds, search.Bounds) {
		t.Fatalf("history toast %+v overlaps search box %+v", toastRegion.Bounds, search.Bounds)
	}
}
