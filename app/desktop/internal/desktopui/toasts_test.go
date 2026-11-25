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
