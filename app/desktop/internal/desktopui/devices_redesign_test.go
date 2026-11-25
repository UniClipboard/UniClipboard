package desktopui

import (
	"bytes"
	"context"
	"encoding/json"
	"image/png"
	"io"
	"math"
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func TestFormatSeenSecondsVsLastActiveMs(t *testing.T) {
	loc := time.FixedZone("CST", 8*3600)
	now := time.Date(2026, 11, 21, 15, 30, 0, 0, loc)

	if got := formatSeen(0); got != "" {
		t.Fatalf("formatSeen(0) = %q, want empty", got)
	}
	if got := formatLastActiveMs(0); got != "" {
		t.Fatalf("formatLastActiveMs(0) = %q, want empty", got)
	}

	cases := []struct {
		name string
		at   time.Time
		want string
	}{
		{"just now", now.Add(-20 * time.Second), "刚刚"},
		{"minutes ago", now.Add(-12 * time.Minute), "12 分钟前"},
		{"earlier today", time.Date(2026, 11, 21, 9, 5, 0, 0, loc), "今天 09:05"},
		{"yesterday", time.Date(2026, 11, 20, 18, 2, 0, 0, loc), "昨天 18:02"},
		{"older", time.Date(2026, 11, 10, 8, 15, 0, 0, loc), "2026-11-10 08:15"},
	}
	for _, tc := range cases {
		if got := formatRelativeAt(tc.at, now); got != tc.want {
			t.Errorf("%s: formatRelativeAt = %q, want %q", tc.name, got, tc.want)
		}
	}

	// Verify seconds vs milliseconds units never produce 1970 dates.
	sec := time.Now().Add(-5 * time.Minute).Unix()
	if got := formatSeen(sec); got != "5 分钟前" && got != "4 分钟前" && got != "6 分钟前" {
		t.Fatalf("formatSeen(Unix seconds) = %q, want ~5 分钟前 (must not treat seconds as ms)", got)
	}
	ms := time.Now().Add(-5 * time.Minute).UnixMilli()
	if got := formatLastActiveMs(ms); got != "5 分钟前" && got != "4 分钟前" && got != "6 分钟前" {
		t.Fatalf("formatLastActiveMs(Unix ms) = %q, want ~5 分钟前", got)
	}
}

func TestDeviceAggregateStatus(t *testing.T) {
	d := &viewState{language: "zh-CN"}
	cases := []struct {
		name   string
		dev    device
		text   string
		tone   desktopkit.StatusTone
		online bool
	}{
		{
			name:   "local",
			dev:    device{id: "local", name: "MacBook"},
			text:   "本机",
			tone:   desktopkit.StatusPositive,
			online: true,
		},
		{
			name:   "no paths",
			dev:    device{id: "spare", name: "备机"},
			text:   "无路径",
			tone:   desktopkit.StatusNeutral,
			online: false,
		},
		{
			name: "only pending path counts as no authorized path",
			dev: device{id: "p", bindings: []deviceBinding{
				{id: "b1", instanceID: "builtin.tailcat", state: "pending", syncEnabled: true, connection: "disconnected"},
			}},
			text:   "无路径",
			tone:   desktopkit.StatusNeutral,
			online: false,
		},
		{
			name: "all paused",
			dev: device{id: "phone", bindings: []deviceBinding{
				{id: "b1", instanceID: "builtin.tailcat", state: "authorized", syncEnabled: false, connection: "connected"},
			}},
			text:   "已暂停",
			tone:   desktopkit.StatusNeutral,
			online: false,
		},
		{
			name: "partial online 1/2",
			dev: device{id: "linux", bindings: []deviceBinding{
				{id: "b1", instanceID: "builtin.tailcat", state: "authorized", syncEnabled: true, connection: "connected"},
				{id: "b2", instanceID: "e2e.work", state: "authorized", syncEnabled: true, connection: "disconnected"},
			}},
			text:   "在线 · 1/2",
			tone:   desktopkit.StatusPositive,
			online: true,
		},
		{
			name: "all online 2/2",
			dev: device{id: "linux2", bindings: []deviceBinding{
				{id: "b1", instanceID: "builtin.tailcat", state: "authorized", syncEnabled: true, connection: "connected"},
				{id: "b2", instanceID: "e2e.work", state: "authorized", syncEnabled: true, connection: "connected"},
			}},
			text:   "在线",
			tone:   desktopkit.StatusPositive,
			online: true,
		},
		{
			name: "offline",
			dev: device{id: "win", bindings: []deviceBinding{
				{id: "b1", instanceID: "builtin.tailcat", state: "authorized", syncEnabled: true, connection: "disconnected"},
			}},
			text:   "离线",
			tone:   desktopkit.StatusNeutral,
			online: false,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			text, tone, online := d.deviceAggregateStatus(tc.dev)
			if text != tc.text || tone != tc.tone || online != tc.online {
				t.Fatalf("got (%q, %v, %v), want (%q, %v, %v)", text, tone, online, tc.text, tc.tone, tc.online)
			}
		})
	}
}

func TestBatchPauseButtonVisibility(t *testing.T) {
	for _, theme := range []string{"light", "dark"} {
		t.Run(theme, func(t *testing.T) {
			d := newDemo("devices", theme)
			tt := ui.NewTesterWith(d.view, 1120, 760, ui.TesterOptions{Dark: theme == "dark"})

			// 0 authorized paths ("spare"): no batch pause button.
			d.deviceList.Value = "spare"
			tt.Frame()
			if tt.HasText("暂停全部路径") || tt.HasText("恢复全部路径") {
				t.Fatal("0-path device must not show batch pause/resume button")
			}

			// 1 authorized path ("phone"): no batch pause button.
			d.deviceList.Value = "phone"
			tt.Frame()
			if tt.HasText("暂停全部路径") || tt.HasText("恢复全部路径") {
				t.Fatal("1-path device must not show batch pause/resume button")
			}

			// 1 authorized + 1 pending path ("windows"): still only 1 authorized path -> no batch button.
			d.deviceList.Value = "windows"
			tt.Frame()
			if tt.HasText("暂停全部路径") || tt.HasText("恢复全部路径") {
				t.Fatal("device with 1 authorized + 1 pending path must not show batch pause/resume button")
			}

			// 2 authorized paths ("linux"): must show batch pause button and toggle cleanly.
			d.deviceList.Value = "linux"
			tt.Frame()
			if !tt.HasText("暂停全部路径") {
				t.Fatal("2-path device missing batch pause button")
			}
			if err := tt.Click("暂停全部路径"); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
			if !tt.HasText("恢复全部路径") {
				t.Fatal("clicking batch pause did not switch to batch resume")
			}
		})
	}
}

func TestPreciseInlineApprovalFiltering(t *testing.T) {
	d := &viewState{
		deviceTask: "invite",
		live: &liveState{
			pairingBackend: "e2e.work",
			taskKnownPending: map[string]int64{
				"old-peer": 3,
			},
		},
		pending: []pendingDevice{
			{id: "old-peer", name: "Old", revision: 3, byCode: true, paths: []pendingPath{{id: "b0", instanceID: "e2e.work", state: "pending"}}},
		},
	}

	// Pre-existing pending request must NOT hijack the pairing sheet.
	if got := d.matchingInlinePending(); got != nil {
		t.Fatalf("pre-existing pending hijacked inline approval: %+v", got)
	}

	// Unrelated new pending on a different backend (not byCode) must NOT hijack either.
	d.pending = append(d.pending, pendingDevice{
		id: "other-be", name: "Other", revision: 1, byCode: false,
		paths: []pendingPath{{id: "b1", instanceID: "builtin.tailcat", state: "pending"}},
	})
	if got := d.matchingInlinePending(); got != nil {
		t.Fatalf("different-backend pending hijacked inline approval: %+v", got)
	}

	// New pending matching active pairing backend MUST match.
	d.pending = append(d.pending, pendingDevice{
		id: "new-peer", name: "New", revision: 1, byCode: false,
		paths: []pendingPath{{id: "b2", instanceID: "e2e.work", state: "pending"}},
	})
	got := d.matchingInlinePending()
	if got == nil || got.id != "new-peer" {
		t.Fatalf("expected new-peer match, got %+v", got)
	}

	// When waiting on join.ID, ONLY join.ID matches.
	d.deviceTask = "wait"
	d.live.join = &api.PairResult{PeerStatus: api.PeerStatus{ID: "other-be"}}
	got = d.matchingInlinePending()
	if got == nil || got.id != "other-be" {
		t.Fatalf("wait mode expected join.ID match (other-be), got %+v", got)
	}
}

func TestTrustedDevicePendingPathRejectUsesRemoveBinding(t *testing.T) {
	var mu sync.Mutex
	var calls []string

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	client := api.NewClient("/nonexistent-in-memory.sock")
	client.WrapTransport(func(_ http.RoundTripper) http.RoundTripper {
		return historySendTransport(func(r *http.Request) (*http.Response, error) {
			mu.Lock()
			calls = append(calls, r.Method+" "+r.URL.RequestURI())
			mu.Unlock()
			return &http.Response{
				StatusCode: http.StatusOK,
				Body:       io.NopCloser(bytes.NewBufferString(`{}`)),
				Header:     make(http.Header),
			}, nil
		})
	})
	d := &viewState{
		devices: []device{
			{id: "local", name: "Local"},
			{id: "trusted-1", name: "Work PC", revision: 5, bindings: []deviceBinding{
				{id: "b-auth", instanceID: "builtin.tailcat", state: "authorized", syncEnabled: true, revision: 2},
				{id: "b-pend", instanceID: "e2e.work", state: "pending", syncEnabled: true, revision: 7},
			}},
		},
		live: &liveState{
			ctx:     ctx,
			cancel:  cancel,
			client:  client,
			jobs:    make(chan func(context.Context) func(), 4),
			results: make(chan func(), 8),
			pending: map[string]api.PendingPeer{
				"trusted-1": {
					ID: "trusted-1", Name: "Work PC", Revision: 5,
					Paths: []engine.DeviceBinding{
						{ID: "b-auth", InstanceID: "builtin.tailcat", State: "authorized", Revision: 2},
						{ID: "b-pend", InstanceID: "e2e.work", State: "pending", Revision: 7},
					},
				},
				"brand-new": {
					ID: "brand-new", Name: "New Laptop", Revision: 1,
					Paths: []engine.DeviceBinding{
						{ID: "b-new", InstanceID: "builtin.tailcat", State: "pending", Revision: 1},
					},
				},
			},
		},
	}
	go d.live.work()

	drain := func(label string) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			select {
			case f := <-d.live.results:
				f()
			default:
			}
			if !d.live.busy {
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		t.Fatalf("timeout waiting for %s", label)
	}

	// 1. Rejecting pending path on trusted device MUST call DELETE /v1/bindings/b-pend?revision=7, NEVER Reject or Unpair.
	d.approveLive("trusted-1", false)
	drain("reject pending path")

	mu.Lock()
	gotCalls := slices.Clone(calls)
	calls = nil
	mu.Unlock()
	if !slices.Equal(gotCalls, []string{"DELETE /v1/bindings/b-pend?revision=7"}) {
		t.Fatalf("trusted pending path reject calls = %v, want [DELETE /v1/bindings/b-pend?revision=7]", gotCalls)
	}

	// 2. Rejecting brand-new pending device MUST call POST /v1/pending/brand-new/reject.
	d.approveLive("brand-new", false)
	drain("reject brand new")

	mu.Lock()
	gotCalls = slices.Clone(calls)
	mu.Unlock()
	if !slices.Equal(gotCalls, []string{"POST /v1/pending/brand-new/reject"}) {
		t.Fatalf("brand-new pending reject calls = %v, want [POST /v1/pending/brand-new/reject]", gotCalls)
	}
}

func TestDevicesPageInteractionsLightAndDark(t *testing.T) {
	for _, theme := range []string{"light", "dark"} {
		t.Run(theme, func(t *testing.T) {
			d := newDemo("devices", theme)
			tt := ui.NewTesterWith(d.view, 1120, 760, ui.TesterOptions{Dark: theme == "dark"})

			// Sync channels status strip and quick jump.
			if !tt.HasText("同步渠道") || !tt.HasText("管理同步渠道") {
				t.Fatal("missing sync channels status strip")
			}
			// No legacy hardcoded placeholder text.
			if tt.HasText("未提供") {
				t.Fatal("legacy placeholder '未提供' leaked into devices view")
			}

			// Clicking a channel chip jumps directly to Settings > Plugins with pluginSettings.id set.
			if err := tt.Click("e2e.work"); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
			if d.page != "settings" || d.settings.Value != "plugins" || d.pluginSettings.id != "e2e.work" {
				t.Fatalf("channel chip jump failed: page=%q settings=%q plugin=%q", d.page, d.settings.Value, d.pluginSettings.id)
			}

			// Return to devices and check global sync disabled warning + one-click re-enable.
			d.page = "devices"
			d.options["sync"] = false
			tt.Frame()
			if !tt.HasText("全局同步已关闭") || !tt.HasText("开启同步") {
				t.Fatal("missing global sync disabled banner")
			}
			if err := tt.Click("开启同步"); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
			if !d.options["sync"] {
				t.Fatal("one-click 开启同步 did not re-enable global sync")
			}

			// External channel inspector renders without fingerprint.
			d.deviceList.Value = "ch:ch-wx"
			tt.Frame()
			if !tt.HasText("外部渠道") || !tt.HasText("收到时自动写入剪贴板") {
				t.Fatal("external channel inspector missing expected controls")
			}

			// Zero running pairing channels renders ContentUnavailable in pairing sheet.
			for i := range d.syncChannels {
				d.syncChannels[i].state = "disabled"
			}
			d.openPairingSheet("", "")
			tt.Frame()
			if !tt.HasText("没有正在运行的可配对同步渠道") || !tt.HasText("前往插件设置") {
				t.Fatal("zero-channel pairing sheet did not show ContentUnavailable guidance")
			}
			if err := tt.Click("前往插件设置"); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
			if d.page != "settings" || d.settings.Value != "plugins" || d.deviceTask != "" {
				t.Fatalf("zero-channel CTA did not navigate to plugins: page=%q settings=%q task=%q", d.page, d.settings.Value, d.deviceTask)
			}
		})
	}
}

func TestLiveErrorMessageMappingAndI18n(t *testing.T) {
	raw, err := os.ReadFile("../i18n/locales.json")
	if err != nil {
		t.Fatal(err)
	}
	var locales map[string]map[string]string
	if err := json.Unmarshal(raw, &locales); err != nil {
		t.Fatal(err)
	}

	reasons := []string{
		"invalid", "incomplete", "expired", "rate-limited",
		"unsupported", "unavailable", "identity_mismatch", "timeout", "conflict",
	}
	seen := map[string]bool{}
	for _, reason := range reasons {
		msg := liveErrorMessage(&api.Error{Reason: reason, Message: "raw"})
		if strings.TrimSpace(msg) == "" {
			t.Fatalf("empty message for reason %q", reason)
		}
		if seen[msg] {
			t.Fatalf("duplicate message %q for reason %q", msg, reason)
		}
		seen[msg] = true
		if locales[msg] == nil {
			t.Fatalf("error message for %q missing from locales.json: %s", reason, msg)
		}
	}
}

func TestDevicesEscapeReturnsToPreviousLayer(t *testing.T) {
	t.Run("sidebar entry without focus returns to history", func(t *testing.T) {
		d := newDemo("history", "light")
		tt := ui.NewTester(d.view, 1120, 760)
		if err := tt.Click("管理设备…"); err != nil {
			t.Fatal(err)
		}
		if d.page != "devices" {
			t.Fatalf("page = %q, want devices", d.page)
		}
		tt.Key(0, ui.KeyEscape)
		if d.page != "history" {
			t.Fatalf("Escape after entering devices did not return to history: page=%q", d.page)
		}
	})

	t.Run("keyboard shortcut entry returns to history", func(t *testing.T) {
		d := newDemo("history", "light")
		tt := ui.NewTester(d.view, 1120, 760)
		tt.Key(ui.Cmd, ui.Key3)
		if d.page != "devices" {
			t.Fatalf("Cmd+3 page = %q, want devices", d.page)
		}
		tt.Key(0, ui.KeyEscape)
		if d.page != "history" {
			t.Fatalf("Escape after Cmd+3 did not return to history: page=%q", d.page)
		}
	})

	t.Run("wide layout after selecting device card returns to history and clears detail flag", func(t *testing.T) {
		d := newDemo("history", "light")
		tt := ui.NewTester(d.view, 1120, 760)
		if err := tt.Click("管理设备…"); err != nil {
			t.Fatal(err)
		}
		if err := tt.Click("Linux 工作站"); err != nil {
			t.Fatal(err)
		}
		if !d.deviceDetailOpen || d.deviceList.Value != "linux" {
			t.Fatalf("clicking card did not select linux: detail=%v val=%q", d.deviceDetailOpen, d.deviceList.Value)
		}
		tt.Key(0, ui.KeyEscape)
		if d.page != "history" || d.deviceDetailOpen {
			t.Fatalf("Escape in wide devices page: page=%q detail=%v, want history/false", d.page, d.deviceDetailOpen)
		}
	})

	t.Run("single pane pops detail then returns to history", func(t *testing.T) {
		d := newDemo("devices", "light")
		tt := ui.NewTester(d.view, 640, 760)
		if err := tt.Click("Linux 工作站"); err != nil {
			t.Fatal(err)
		}
		if !d.deviceDetailOpen {
			t.Fatal("single-pane card click did not open detail")
		}
		tt.Key(0, ui.KeyEscape)
		if d.page != "devices" || d.deviceDetailOpen {
			t.Fatalf("first Escape in single-pane detail: page=%q detail=%v, want devices/false", d.page, d.deviceDetailOpen)
		}
		tt.Key(0, ui.KeyEscape)
		if d.page != "history" {
			t.Fatalf("second Escape in single-pane overview: page=%q, want history", d.page)
		}
	})

	t.Run("inline rename cancels before leaving devices", func(t *testing.T) {
		d := newDemo("devices", "light")
		tt := ui.NewTester(d.view, 1120, 760)
		d.deviceList.Value = "linux"
		tt.Frame()
		if err := tt.Click("重命名"); err != nil {
			t.Fatal(err)
		}
		if !d.renaming {
			t.Fatal("rename did not open")
		}
		// Escape while input is focused cancels rename only.
		tt.Key(0, ui.KeyEscape)
		if d.renaming || d.page != "devices" {
			t.Fatalf("Escape with focused rename input: renaming=%v page=%q", d.renaming, d.page)
		}
		// Reopen rename and move focus away from input; Escape still cancels rename only.
		if err := tt.Click("重命名"); err != nil {
			t.Fatal(err)
		}
		tt.Key(0, ui.KeyTab) // focus moves off the text input
		tt.Key(0, ui.KeyEscape)
		if d.renaming || d.page != "devices" {
			t.Fatalf("Escape with unfocused rename box: renaming=%v page=%q", d.renaming, d.page)
		}
		// Subsequent Escape leaves devices page.
		tt.Key(0, ui.KeyEscape)
		if d.page != "history" {
			t.Fatalf("Escape after closing rename: page=%q, want history", d.page)
		}
	})

	t.Run("path menu and dialogs dismiss before leaving devices", func(t *testing.T) {
		d := newDemo("devices", "light")
		tt := ui.NewTester(d.view, 1120, 760)
		d.deviceList.Value = "linux"
		tt.Frame()

		// Path overflow menu
		if err := tt.Click("更多操作"); err != nil {
			t.Fatal(err)
		}
		if !tt.HasText("移除此路径") {
			t.Fatal("path menu did not open")
		}
		tt.Key(0, ui.KeyEscape)
		if d.page != "devices" || tt.HasText("移除此路径") {
			t.Fatalf("Escape with menu open: page=%q menuOpen=%v", d.page, tt.HasText("移除此路径"))
		}

		// Confirm dialog
		if err := tt.Click("移除设备"); err != nil {
			t.Fatal(err)
		}
		if d.confirm == "" {
			t.Fatal("confirm dialog did not open")
		}
		tt.Key(0, ui.KeyEscape)
		if d.page != "devices" || d.confirm != "" {
			t.Fatalf("Escape with confirm dialog open: page=%q confirm=%q", d.page, d.confirm)
		}

		// Pairing sheet
		if err := tt.Click("添加设备"); err != nil {
			t.Fatal(err)
		}
		tt.Advance(350 * time.Millisecond)
		if d.deviceTask == "" {
			t.Fatal("pairing sheet did not open")
		}
		tt.Key(0, ui.KeyEscape)
		tt.Advance(350 * time.Millisecond)
		if d.page != "devices" || d.deviceTask != "" {
			t.Fatalf("Escape with pairing sheet open: page=%q task=%q", d.page, d.deviceTask)
		}

		// Now Escape leaves devices page cleanly.
		tt.Key(0, ui.KeyEscape)
		if d.page != "history" {
			t.Fatalf("final Escape did not return to history: page=%q", d.page)
		}
	})
}

func TestDeviceCardsTwoColumnAndSingleColumnAlignment(t *testing.T) {
	// Odd device count (3 cards: local + 2 peers, matching a 2+1 two-column grid)
	// with different content lengths and a selected card on row 2.
	makeState := func() *viewState {
		d := newDemo("devices", "light")
		d.pending = nil
		d.channels = nil
		d.candidates = nil
		d.syncChannels = []syncChannel{
			{id: "builtin.tailcat", implementationID: "builtin.tailcat", state: "running", pairing: true},
			{id: "unic.wxime", implementationID: "unic.wxime", state: "running", externalChannels: true},
		}
		d.devices = []device{
			{id: "local", name: "LNG-8XN9C54", icon: desktopkit.IconDesktop},
			{
				id: "peer-1", name: "Mac-mini.local", icon: desktopkit.IconDesktop, lastSeen: "今天 10:57",
				bindings: []deviceBinding{{id: "b1", instanceID: "builtin.tailcat", implementationID: "builtin.tailcat", state: "authorized", syncEnabled: true, connection: "disconnected"}},
			},
			{
				id: "peer-2", name: "M02 Physical", icon: desktopkit.IconDesktop, lastSeen: "今天 13:18",
				bindings: []deviceBinding{{id: "b2", instanceID: "builtin.tailcat", implementationID: "builtin.tailcat", state: "authorized", syncEnabled: true, connection: "disconnected"}},
			},
		}
		d.deviceList.Value = "peer-2"
		return d
	}

	t.Run("two-column odd count aligns left column and equalizes widths", func(t *testing.T) {
		d := makeState()
		tt := ui.NewTesterWith(d.view, 1120, 760, ui.TesterOptions{})
		tt.Frame()

		c0, ok0 := tt.AccessByRole(ui.RoleListItem, "LNG-8XN9C54")
		c1, ok1 := tt.AccessByRole(ui.RoleListItem, "Mac-mini.local")
		c2, ok2 := tt.AccessByRole(ui.RoleListItem, "M02 Physical")
		if !ok0 || !ok1 || !ok2 {
			t.Fatalf("missing device cards: ok0=%v ok1=%v ok2=%v", ok0, ok1, ok2)
		}

		// Row 1 cards must share identical width and top Y, separated by the 11px gap.
		if math.Abs(float64(c0.Bounds.Y-c1.Bounds.Y)) > 0.5 {
			t.Fatalf("row 1 Y mismatch: c0=%+v c1=%+v", c0.Bounds, c1.Bounds)
		}
		if math.Abs(float64(c0.Bounds.W-c1.Bounds.W)) > 0.5 {
			t.Fatalf("row 1 width mismatch: c0=%+v c1=%+v", c0.Bounds, c1.Bounds)
		}
		// Row 2 lone card must match column 0 X, width, and height exactly.
		if math.Abs(float64(c2.Bounds.X-c0.Bounds.X)) > 0.5 {
			t.Fatalf("row 2 X mismatch with column 0: c0=%+v c2=%+v", c0.Bounds, c2.Bounds)
		}
		if math.Abs(float64(c2.Bounds.W-c0.Bounds.W)) > 0.5 {
			t.Fatalf("row 2 lone card width mismatch with column 0: c0=%+v c2=%+v", c0.Bounds, c2.Bounds)
		}
		if math.Abs(float64(c2.Bounds.H-c0.Bounds.H)) > 0.5 {
			t.Fatalf("selected card height mismatch with unselected card: c0=%+v c2=%+v", c0.Bounds, c2.Bounds)
		}
	})

	t.Run("single-column cards share identical X, width, and height", func(t *testing.T) {
		d := makeState()
		tt := ui.NewTesterWith(d.view, 860, 760, ui.TesterOptions{})
		tt.Frame()

		c0, ok0 := tt.AccessByRole(ui.RoleListItem, "LNG-8XN9C54")
		c1, ok1 := tt.AccessByRole(ui.RoleListItem, "Mac-mini.local")
		c2, ok2 := tt.AccessByRole(ui.RoleListItem, "M02 Physical")
		if !ok0 || !ok1 || !ok2 {
			t.Fatalf("missing single-column device cards: ok0=%v ok1=%v ok2=%v", ok0, ok1, ok2)
		}
		for _, pair := range [][2]ui.Rect{{c0.Bounds, c1.Bounds}, {c0.Bounds, c2.Bounds}} {
			if math.Abs(float64(pair[0].X-pair[1].X)) > 0.5 || math.Abs(float64(pair[0].W-pair[1].W)) > 0.5 {
				t.Fatalf("single-column alignment mismatch: a=%+v b=%+v", pair[0], pair[1])
			}
			if math.Abs(float64(pair[0].H-pair[1].H)) > 0.5 {
				t.Fatalf("single-column height mismatch: a=%+v b=%+v", pair[0], pair[1])
			}
		}
	})
}

func TestBindingPolicyPlanAInteractionsAndLivePayload(t *testing.T) {
	t.Run("demo plan A presets, per-type directions, upper ceiling and reset", func(t *testing.T) {
		for _, theme := range []string{"light", "dark"} {
			t.Run(theme, func(t *testing.T) {
				d := newDemo("devices", theme)
				d.deviceList.Value = "phone"
				// Make phone active and connected with default bidirectional policy.
				for i := range d.devices {
					if d.devices[i].id == "phone" {
						d.devices[i].bindings[0].syncEnabled = true
						d.devices[i].bindings[0].policy = engine.DefaultBindingSyncPolicy()
					}
				}
				tt := ui.NewTesterWith(d.view, 1120, 760, ui.TesterOptions{Dark: theme == "dark"})
				tt.Frame()

				// Single binding defaults expanded, showing Plan A controls.
				if !tt.HasText("快捷模式") || !tt.HasText("本机 → 对端") || !tt.HasText("对端 → 本机") {
					t.Fatal("binding policy Plan A controls missing")
				}

				// Click per-type controls: Text -> 仅发, Images -> 仅收, Files -> 关闭.
				if err := tt.Click("文本：仅发"); err != nil {
					t.Fatal(err)
				}
				if err := tt.Click("图片：仅收"); err != nil {
					t.Fatal(err)
				}
				if err := tt.Click("文件：关闭"); err != nil {
					t.Fatal(err)
				}
				tt.Frame()

				dev := d.deviceByID("phone")
				want := engine.BindingSyncPolicy{
					Text:   engine.SyncDirectionSendOnly,
					Images: engine.SyncDirectionReceiveOnly,
					Files:  engine.SyncDirectionDisabled,
				}
				if dev.bindings[0].policy != want {
					t.Fatalf("policy = %+v, want %+v", dev.bindings[0].policy, want)
				}
				if !tt.HasText("自定义") {
					t.Fatal("customized badge not shown")
				}

				// Upper-layer channel ceiling locks Files when channel disables files.
				for i := range d.syncChannels {
					if d.syncChannels[i].id == "builtin.tailcat" {
						d.syncChannels[i].noFiles = true
					}
				}
				tt.Frame()
				if !tt.HasText("渠道已关闭") {
					t.Fatal("upper-layer channel lock badge not shown")
				}
				for i := range d.syncChannels {
					if d.syncChannels[i].id == "builtin.tailcat" {
						d.syncChannels[i].noFiles = false
					}
				}

				// Quick preset "仅发送" sets all three types to send_only.
				if err := tt.Click("仅发送"); err != nil {
					t.Fatal(err)
				}
				tt.Frame()
				wantSend := engine.BindingSyncPolicy{
					Text:   engine.SyncDirectionSendOnly,
					Images: engine.SyncDirectionSendOnly,
					Files:  engine.SyncDirectionSendOnly,
				}
				if dev.bindings[0].policy != wantSend {
					t.Fatalf("after 仅发送 preset: %+v, want %+v", dev.bindings[0].policy, wantSend)
				}
			})
		}
	})

	t.Run("live policy update sends PATCH /v1/bindings with policy payload", func(t *testing.T) {
		var mu sync.Mutex
		var gotMethod, gotURI, gotBody string

		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		client := api.NewClient("/nonexistent-in-memory.sock")
		client.WrapTransport(func(_ http.RoundTripper) http.RoundTripper {
			return historySendTransport(func(r *http.Request) (*http.Response, error) {
				raw, _ := io.ReadAll(r.Body)
				mu.Lock()
				gotMethod = r.Method
				gotURI = r.URL.RequestURI()
				gotBody = string(raw)
				mu.Unlock()
				return &http.Response{
					StatusCode: http.StatusOK,
					Body:       io.NopCloser(bytes.NewBufferString(`{}`)),
					Header:     make(http.Header),
				}, nil
			})
		})
		d := &viewState{
			live: &liveState{
				ctx:     ctx,
				cancel:  cancel,
				client:  client,
				jobs:    make(chan func(context.Context) func(), 4),
				results: make(chan func(), 8),
			},
		}
		go d.live.work()

		pol := engine.BindingSyncPolicy{
			Text:   engine.SyncDirectionBidirectional,
			Images: engine.SyncDirectionSendOnly,
			Files:  engine.SyncDirectionDisabled,
		}
		d.setLiveBindingPolicy("b-42", 7, nil, pol)

		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			select {
			case f := <-d.live.results:
				f()
			default:
			}
			if !d.live.busy {
				break
			}
			time.Sleep(10 * time.Millisecond)
		}

		mu.Lock()
		defer mu.Unlock()
		if gotMethod != http.MethodPatch || gotURI != "/v1/bindings/b-42" {
			t.Fatalf("request = %s %s, want PATCH /v1/bindings/b-42", gotMethod, gotURI)
		}
		var edit api.BindingEdit
		if err := json.Unmarshal([]byte(gotBody), &edit); err != nil {
			t.Fatalf("invalid json %q: %v", gotBody, err)
		}
		if edit.Revision != 7 || edit.Enabled != nil || edit.Policy == nil || *edit.Policy != pol {
			t.Fatalf("payload = %+v, want rev=7 enabled=nil policy=%+v", edit, pol)
		}
	})
}

func TestCaptureDevicePolicyEvidence(t *testing.T) {
	outDir := os.Getenv("UNIC_DEVICE_POLICY_EVIDENCE_DIR")
	if outDir == "" {
		t.Skip("set UNIC_DEVICE_POLICY_EVIDENCE_DIR to export screenshots")
	}
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		t.Fatal(err)
	}
	save := func(name string, tt *ui.Tester) {
		t.Helper()
		f, err := os.Create(filepath.Join(outDir, name))
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = f.Close() }()
		if err := png.Encode(f, tt.Image()); err != nil {
			t.Fatal(err)
		}
	}
	makeScreenshotState := func(theme string) *viewState {
		d := newDemo("devices", theme)
		d.pending = nil
		d.syncChannels = []syncChannel{
			{id: "builtin.tailcat", implementationID: "builtin.tailcat", state: "running", pairing: true},
			{id: "unic.wxime", implementationID: "unic.wxime", state: "running", externalChannels: true},
		}
		d.devices = []device{
			{id: "local", name: "LNG-8XN9C54", icon: desktopkit.IconDesktop},
			{
				id: "peer-1", name: "Mac-mini.local", icon: desktopkit.IconDesktop, lastSeen: "今天 10:57",
				fingerprint: "8A42 7C91 2F60 B3D8 41E5 9A0C 7D12 5F3E",
				bindings: []deviceBinding{{
					id: "b1", instanceID: "builtin.tailcat", implementationID: "builtin.tailcat",
					state: "authorized", syncEnabled: true, connection: "disconnected",
					lastActive: "今天 10:57", policy: engine.DefaultBindingSyncPolicy(),
				}},
			},
			{
				id: "peer-2", name: "M02 Physical", icon: desktopkit.IconDesktop, lastSeen: "今天 13:18",
				fingerprint: "BE67 5F51 6005 18C8 1D3B BFEE DA2E 8F2B",
				bindings: []deviceBinding{{
					id: "b2", instanceID: "builtin.tailcat", implementationID: "builtin.tailcat",
					state: "authorized", syncEnabled: true, connection: "disconnected",
					lastActive: "今天 13:18",
					policy: engine.BindingSyncPolicy{
						Text:   engine.SyncDirectionBidirectional,
						Images: engine.SyncDirectionSendOnly,
						Files:  engine.SyncDirectionDisabled,
					},
				}},
			},
		}
		d.channels = []externalChannel{
			{id: "ch-wetype", name: "WeType", backendID: "unic.wxime", enabled: true, autoApply: true},
		}
		d.candidates = []deviceCandidate{
			{id: "cand-1", name: "MARKEA46", introducer: "Mac-mini.local", backendID: "builtin.tailcat", icon: desktopkit.IconDesktop},
		}
		d.deviceList.Value = "peer-2"
		return d
	}

	// 1. Light mode: exact 3-device scenario with aligned 2-col cards + expanded Plan A policy.
	dLight := makeScreenshotState("light")
	ttLight := ui.NewTesterWith(dLight.view, 1120, 760, ui.TesterOptions{Dark: false})
	ttLight.SetScale(2)
	ttLight.Frame()
	save("01-devices-light-aligned-and-policy.png", ttLight)

	// 2. Dark mode: upper-layer channel ceiling lock on Files + custom policy.
	dDark := makeScreenshotState("dark")
	dDark.syncChannels[0].noFiles = true
	dDark.devices[2].bindings[0].policy = engine.BindingSyncPolicy{
		Text:   engine.SyncDirectionReceiveOnly,
		Images: engine.SyncDirectionBidirectional,
		Files:  engine.SyncDirectionBidirectional,
	}
	ttDark := ui.NewTesterWith(dDark.view, 1120, 760, ui.TesterOptions{Dark: true})
	ttDark.SetScale(2)
	ttDark.Frame()
	save("02-devices-dark-channel-ceiling.png", ttDark)

	// 3. Multi-channel device: 2 independent channels on one device (one expanded, one collapsed with summary).
	dMulti := newDemo("devices", "light")
	dMulti.deviceList.Value = "linux"
	for i := range dMulti.devices {
		if dMulti.devices[i].id == "linux" && len(dMulti.devices[i].bindings) >= 2 {
			dMulti.devices[i].bindings[0].policy = engine.BindingSyncPolicy{
				Text:   engine.SyncDirectionSendOnly,
				Images: engine.SyncDirectionSendOnly,
				Files:  engine.SyncDirectionSendOnly,
			}
			dMulti.devices[i].bindings[1].policy = engine.BindingSyncPolicy{
				Text:   engine.SyncDirectionBidirectional,
				Images: engine.SyncDirectionReceiveOnly,
				Files:  engine.SyncDirectionDisabled,
			}
			dMulti.expandedBindings = map[string]bool{
				dMulti.devices[i].bindings[0].id: true,
				dMulti.devices[i].bindings[1].id: false,
			}
		}
	}
	ttMulti := ui.NewTesterWith(dMulti.view, 1120, 760, ui.TesterOptions{Dark: false})
	ttMulti.SetScale(2)
	ttMulti.Frame()
	save("03-devices-multi-channel-policy.png", ttMulti)
}
