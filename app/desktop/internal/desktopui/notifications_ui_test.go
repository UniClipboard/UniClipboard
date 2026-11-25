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

func TestPendingDeviceSidebarBadgeGuidanceToastAndNavigation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	d := newDemo("history", "light")
	d.pending = []pendingDevice{
		{
			id:          "dev-m02",
			name:        "M02 Physical",
			byCode:      true,
			fingerprint: "8F18 D856 C0E4 D7DF",
			revision:    1,
			icon:        desktopkit.IconDesktop,
			paths:       []pendingPath{{id: "bp-1", instanceID: "builtin.tailcat", state: "pending", revision: 1}},
		},
	}
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
		pending: map[string]api.PendingPeer{
			"dev-m02": {ID: "dev-m02", Name: "M02 Physical", ByCode: true, Fingerprint: "8F18 D856 C0E4 D7DF", Revision: 1},
		},
		config: config.State{Revision: "rev-1", Config: config.Default()},
	}
	screen := &Desktop{d: d}

	var badgeCounts []int
	screen.SetPendingCountChanged(func(n int) {
		badgeCounts = append(badgeCounts, n)
	})

	tt := ui.NewTesterWith(screen.View, 1100, 720, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	tt.Frame()

	// Dock/app badge callback received count=1.
	if len(badgeCounts) == 0 || badgeCounts[len(badgeCounts)-1] != 1 {
		t.Fatalf("badgeCounts = %v, want last=1", badgeCounts)
	}

	// Sidebar footer entry preserves accessible label and shows numeric badge.
	if _, ok := tt.AccessByRole(ui.RoleListItem, "管理设备…"); !ok {
		t.Fatalf("missing 管理设备… sidebar item: %v", tt.Texts())
	}
	if !tt.HasText("1") {
		t.Fatalf("missing pending badge count in sidebar: %v", tt.Texts())
	}

	// In-app guidance toast appears on history page with actionable CTA.
	if !tt.HasText("M02 Physical 请求加入同步网络") {
		t.Fatalf("missing pending guidance toast title: %v", tt.Texts())
	}
	if !tt.HasText("前往核对") {
		t.Fatalf("missing pending guidance CTA: %v", tt.Texts())
	}

	// Clicking 前往核对 jumps directly to Devices and focuses the pending item.
	if err := tt.Click("前往核对"); err != nil {
		t.Fatalf("click 前往核对: %v", err)
	}
	tt.Frame()
	if d.page != "devices" {
		t.Fatalf("page = %q, want devices", d.page)
	}
	if d.deviceList.Value != "pending:dev-m02" || !d.deviceDetailOpen {
		t.Fatalf("device selection = (%q, detailOpen=%v), want (pending:dev-m02, true)", d.deviceList.Value, d.deviceDetailOpen)
	}
	// On the devices page, the guidance toast is dismissed automatically.
	if tt.HasText("前往核对") {
		t.Fatal("guidance toast remained visible on devices page")
	}

	// Clearing pending items updates badge count to 0.
	d.pending = nil
	d.live.pending = map[string]api.PendingPeer{}
	tt.Frame()
	if badgeCounts[len(badgeCounts)-1] != 0 {
		t.Fatalf("badgeCounts after clear = %v, want last=0", badgeCounts)
	}
}

func TestNotificationSettingsPageAndRoutes(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	d := newDemo("history", "light")
	d.pending = nil
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

	readCalled := make(chan struct{}, 4)
	openCalled := make(chan struct{}, 1)
	testCalled := make(chan struct{}, 1)
	screen.SetNotificationActions(
		func(context.Context) (string, error) {
			select {
			case readCalled <- struct{}{}:
			default:
			}
			return "denied", nil
		},
		func(context.Context) error {
			select {
			case openCalled <- struct{}{}:
			default:
			}
			return nil
		},
		func(context.Context) error {
			select {
			case testCalled <- struct{}{}:
			default:
			}
			return nil
		},
	)

	// OpenFromPanel routes straight to settings:notifications.
	screen.OpenFromPanel("settings:notifications")
	tt := ui.NewTesterWith(screen.View, 1100, 720, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	tt.Frame()

	select {
	case <-readCalled:
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for passive notification status read")
	}
	deadline := time.Now().Add(2 * time.Second)
	for !d.notifications.loaded && time.Now().Before(deadline) {
		tt.Frame()
		time.Sleep(10 * time.Millisecond)
	}
	tt.Frame()

	if !tt.HasText("通知开关") || !tt.HasText("系统通知权限") || !tt.HasText("未授权") {
		t.Fatalf("missing notification settings content: %v", tt.Texts())
	}

	if err := tt.Click("发送测试通知"); err != nil {
		t.Fatalf("click 发送测试通知: %v", err)
	}
	select {
	case <-testCalled:
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for test notification call")
	}

	if err := tt.Click("打开系统设置"); err != nil {
		t.Fatalf("click 打开系统设置: %v", err)
	}
	select {
	case <-openCalled:
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for open notification settings call")
	}

	if snapDir := os.Getenv("UNIC_NOTIF_SNAP_DIR"); snapDir != "" {
		_ = os.MkdirAll(snapDir, 0o755)
		savePNG := func(name string, tester *ui.Tester) {
			f, err := os.Create(filepath.Join(snapDir, name))
			if err != nil {
				t.Fatalf("create %s: %v", name, err)
			}
			defer func() { _ = f.Close() }()
			if err := png.Encode(f, tester.Image()); err != nil {
				t.Fatalf("encode %s: %v", name, err)
			}
		}

		// 1. History page (dark) with red badge on 管理设备… and pending guidance toast.
		sd := newDemo("history", "dark")
		sd.pending = []pendingDevice{
			{
				id:          "dev-m02",
				name:        "M02 Physical",
				byCode:      true,
				fingerprint: "8F18 D856 C0E4 D7DF D6BD C71F 5D28 B72C",
				revision:    1,
				icon:        desktopkit.IconDesktop,
				paths:       []pendingPath{{id: "bp-1", instanceID: "builtin.tailcat", state: "pending", revision: 1}},
			},
		}
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
			pending: map[string]api.PendingPeer{
				"dev-m02": {ID: "dev-m02", Name: "M02 Physical", ByCode: true, Fingerprint: "8F18 D856 C0E4 D7DF D6BD C71F 5D28 B72C", Revision: 1},
			},
			config: config.State{Revision: "rev-1", Config: config.Default()},
		}
		sc := &Desktop{d: sd}
		st := ui.NewTesterWith(sc.View, 1100, 720, ui.TesterOptions{Dark: true})
		st.SetScale(2)
		st.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
		st.Advance(350 * time.Millisecond)
		savePNG("01-history-pending-badge-toast-dark.png", st)

		// 2. History page (dark) with collapsed sidebar showing corner red dot on toggle.
		if err := st.Click("切换侧栏"); err != nil {
			t.Fatalf("click 切换侧栏: %v", err)
		}
		st.Advance(400 * time.Millisecond)
		st.Frame()
		savePNG("02-history-collapsed-sidebar-dot-dark.png", st)

		// 3. After clicking 前往核对 -> Devices page with pending device focused on the right.
		if err := st.Click("前往核对"); err != nil {
			t.Fatalf("click 前往核对: %v", err)
		}
		st.Frame()
		st.Advance(350 * time.Millisecond)
		savePNG("03-devices-focused-pending-dark.png", st)

		// 4. Settings -> Notifications (dark, granted status) and (light, denied warning status).
		for _, tc := range []struct {
			name   string
			theme  string
			dark   bool
			status string
		}{
			{"04-settings-notifications-dark.png", "dark", true, "granted"},
			{"05-settings-notifications-light-denied.png", "light", false, "denied"},
		} {
			nd := newDemo("settings", tc.theme)
			nd.pending = nil
			nd.settings.Value = "notifications"
			nd.live = &liveState{
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
			nsc := &Desktop{d: nd}
			nsc.SetNotificationActions(
				func(context.Context) (string, error) { return tc.status, nil },
				func(context.Context) error { return nil },
				func(context.Context) error { return nil },
			)
			nt := ui.NewTesterWith(nsc.View, 1100, 720, ui.TesterOptions{Dark: tc.dark})
			nt.SetScale(2)
			nt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
			waitUntil := time.Now().Add(2 * time.Second)
			for !nd.notifications.loaded && time.Now().Before(waitUntil) {
				nt.Frame()
				time.Sleep(10 * time.Millisecond)
			}
			nt.Advance(350 * time.Millisecond)
			savePNG(tc.name, nt)
		}
	}
}
