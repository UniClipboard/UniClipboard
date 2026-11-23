package desktopui

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func newRestartNoticeTestDesktop(t *testing.T) (*Desktop, context.CancelFunc) {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	d := &viewState{
		page:       "settings",
		theme:      "light",
		layout:     "list",
		settings:   desktopkit.NavState{Value: "plugins"},
		deviceList: desktopkit.SourceState{Value: "local"},
		options:    map[string]bool{},
		choices:    map[string]string{},
	}
	d.live = &liveState{
		ctx:               ctx,
		cancel:            cancel,
		client:            api.NewClient(filepath.Join(t.TempDir(), "unused.sock")),
		loaded:            true,
		loading:           true, // suppress background refresh in unit test
		nextRefresh:       time.Now().Add(time.Hour),
		results:           make(chan func(), 32),
		jobs:              make(chan func(context.Context) func(), 16),
		items:             map[string]api.HistoryItem{},
		details:           map[string]entry{},
		pending:           map[string]api.PendingPeer{},
		config:            config.State{Revision: "rev-1", Config: config.Default(), RestartRequired: []string{"plugins"}},
		marketplaceLoaded: true,
	}
	return &Desktop{d: d}, cancel
}

func TestRestartWarningButtonInPluginsSettings(t *testing.T) {
	screen, cancel := newRestartNoticeTestDesktop(t)
	defer cancel()

	// Without a host restarter, the warning notice renders without a restart button.
	tt := ui.NewTester(screen.View, 1100, 720)
	if !tt.HasText("插件设置已保存。请退出并重新启动桌面端以生效。") {
		t.Fatalf("plugin restart notice missing: %v", tt.Texts())
	}
	if _, ok := tt.AccessByRole(ui.RoleButton, "立即重启"); ok {
		t.Fatal("restart button appeared without host restarter")
	}

	// Bind a host restarter and verify click, busy/disabled state, and error recovery.
	var calls int
	var pendingDone func(error)
	screen.SetRestarter(func(restartService bool, done func(error)) {
		if !restartService {
			t.Fatal("plugin changes must request a service restart")
		}
		calls++
		pendingDone = done
	})
	tt.Frame()

	btn, ok := tt.AccessByRole(ui.RoleButton, "立即重启")
	if !ok {
		t.Fatal("restart button missing in plugins restart warning")
	}
	if btn.Disabled || btn.Busy {
		t.Fatalf("expected enabled restart button, got disabled=%v busy=%v", btn.Disabled, btn.Busy)
	}
	if err := tt.Click("立即重启"); err != nil {
		t.Fatalf("click restart button: %v", err)
	}
	if calls != 1 || pendingDone == nil {
		t.Fatalf("restart callback calls = %d, want 1", calls)
	}
	tt.Frame()
	btn, ok = tt.AccessByRole(ui.RoleButton, "立即重启")
	if !ok || !btn.Disabled || !btn.Busy {
		t.Fatalf("expected busy+disabled restart button while restarting, got ok=%v disabled=%v busy=%v", ok, btn.Disabled, btn.Busy)
	}
	// A second click while in-flight must not trigger another restart.
	_ = tt.Click("立即重启")
	if calls != 1 {
		t.Fatalf("duplicate restart callback while busy: %d", calls)
	}

	// Simulate IPC failure: button re-enables and error notice becomes visible.
	pendingDone(errors.New("tray unavailable"))
	tt.Frame()
	if !tt.HasText("无法重启应用。请手动退出并重新启动桌面端。") {
		t.Fatalf("restart failure notice missing: %v", tt.Texts())
	}
	btn, ok = tt.AccessByRole(ui.RoleButton, "立即重启")
	if !ok || btn.Disabled || btn.Busy {
		t.Fatalf("expected re-enabled restart button after failure, got ok=%v disabled=%v busy=%v", ok, btn.Disabled, btn.Busy)
	}

	// Simulate success: callback invoked once and remains busy until host closes GUI.
	pendingDone = nil
	if err := tt.Click("立即重启"); err != nil {
		t.Fatalf("retry click restart button: %v", err)
	}
	if calls != 2 || pendingDone == nil {
		t.Fatalf("retry restart callback calls = %d, want 2", calls)
	}
	pendingDone(nil)
	tt.Frame()
	if tt.HasText("无法重启应用。请手动退出并重新启动桌面端。") {
		t.Fatal("stale restart failure notice remained after retry")
	}
}

func TestRestartWarningButtonInGeneralSettingsAndDevices(t *testing.T) {
	screen, cancel := newRestartNoticeTestDesktop(t)
	defer cancel()

	var calls int
	screen.SetRestarter(func(restartService bool, done func(error)) {
		if restartService != (calls > 0) {
			t.Fatalf("unexpected service restart scope on click %d: %v", calls+1, restartService)
		}
		calls++
		done(errors.New("retryable"))
	})

	// 1. Settings -> Appearance with windowFrame mismatch (host-only restart).
	frame := false
	screen.d.windowFrame = &frame
	screen.d.live.config.RestartRequired = nil
	screen.d.live.config.Config.Desktop.SystemWindowFrame = true
	screen.d.settings.Value = "appearance"

	tt := ui.NewTester(screen.View, 1100, 720)
	if !tt.HasText("部分设置将在下次启动时生效。") {
		t.Fatalf("settings restart notice missing: %v", tt.Texts())
	}
	if err := tt.Click("立即重启"); err != nil {
		t.Fatalf("click appearance restart button: %v", err)
	}
	if calls != 1 {
		t.Fatalf("appearance restart calls = %d, want 1", calls)
	}
	tt.Frame()

	// 2. Devices view sync-channel strip (both running and zero-channel states).
	screen.d.page = "devices"
	screen.d.restartPending = true
	screen.d.syncChannels = []syncChannel{{id: "builtin.tailcat", implementationID: "builtin.tailcat", state: "running", restart: true}}
	tt.Frame()
	if !tt.HasText("同步渠道设置已保存，请退出并重新启动桌面端以生效。") {
		t.Fatalf("running-channel restart notice missing: %v", tt.Texts())
	}
	if err := tt.Click("立即重启"); err != nil {
		t.Fatalf("click running-channel restart button: %v", err)
	}
	if calls != 2 {
		t.Fatalf("running-channel restart calls = %d, want 2", calls)
	}
	tt.Frame()

	screen.d.syncChannels = []syncChannel{{id: "builtin.tailcat", implementationID: "builtin.tailcat", state: "disabled", restart: true}}
	tt.Frame()
	if !tt.HasText("同步渠道配置已更改，请退出并重新启动桌面端以生效。") {
		t.Fatalf("zero-channel restart notice missing: %v", tt.Texts())
	}
	if err := tt.Click("立即重启"); err != nil {
		t.Fatalf("click zero-channel restart button: %v", err)
	}
	if calls != 3 {
		t.Fatalf("zero-channel restart calls = %d, want 3", calls)
	}
}
