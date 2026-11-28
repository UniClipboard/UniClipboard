package desktopui

import (
	"context"
	"crypto/sha256"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	"github.com/uniclipboard/unic/engine"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func scenarioSettingsNotificationRefresh(t *testing.T) {
	for _, tc := range []struct {
		language string
		width    int
	}{
		{language: "zh-CN", width: 1100},
		{language: "en", width: 640},
	} {
		t.Run(tc.language, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			d := newDemo("settings", "light")
			d.pending = nil
			cfg := config.Default()
			cfg.Desktop.Language = tc.language
			d.live = &liveState{
				ctx: ctx, loaded: true, loading: true, key: d.liveQueryKey(),
				config: config.State{Config: cfg}, results: make(chan func(), 8),
				catalogueBase: map[string][]engine.HistorySearchCandidate{},
			}
			screen := &Desktop{d: d}
			type result struct {
				status string
				err    error
			}
			responses := make(chan result, 1)
			openGate := make(chan error, 1)
			sendGate := make(chan error, 1)
			var reads atomic.Int32
			var openCalls atomic.Int32
			var sendCalls atomic.Int32
			screen.SetNotificationActions(
				func(ctx context.Context) (string, error) {
					reads.Add(1)
					select {
					case reply := <-responses:
						return reply.status, reply.err
					case <-ctx.Done():
						return "", ctx.Err()
					}
				},
				func(ctx context.Context) error {
					openCalls.Add(1)
					select {
					case err := <-openGate:
						return err
					case <-ctx.Done():
						return ctx.Err()
					}
				},
				func(ctx context.Context) error {
					sendCalls.Add(1)
					select {
					case err := <-sendGate:
						return err
					case <-ctx.Done():
						return ctx.Err()
					}
				},
			)
			screen.OpenFromPanel("settings:notifications")
			view := ui.NewTesterWith(screen.View, tc.width, 720, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
			proof.Capture(t, view)

			// Initial fast load must not flash "状态未知" before the first permission arrives.
			if view.HasText(d.tr("状态未知")) {
				t.Errorf("initial pending read flashed unknown status before 120ms: %v", view.Texts())
			}

			wait := func(condition func() bool) {
				t.Helper()
				deadline := time.Now().Add(2 * time.Second)
				for !condition() {
					if time.Now().After(deadline) {
						t.Fatal("timed out waiting for the notification worker")
					}
					time.Sleep(time.Millisecond)
				}
			}
			drain := func() {
				t.Helper()
				wait(func() bool { return len(d.live.results) > 0 })
				view.Frame()
			}
			complete := func(reply result) {
				t.Helper()
				responses <- reply
				drain()
			}
			complete(result{status: "denied"})

			refresh, send, open := d.tr("刷新权限状态"), d.tr("发送测试通知"), d.tr("打开系统设置")
			button := func(name string) ui.AccessInfo {
				t.Helper()
				node, found := view.AccessByRole(ui.RoleButton, name)
				if !found || node.Offscreen || node.Bounds.W <= 0 || node.Bounds.H <= 0 {
					t.Fatalf("button %q is not visible: %+v", name, node)
				}
				return node
			}
			bounds := func() [3]ui.Rect {
				return [3]ui.Rect{button(refresh).Bounds, button(send).Bounds, button(open).Bounds}
			}
			initial := bounds()
			regionHash := func(rects ...ui.Rect) [32]byte {
				img := view.Image()
				hash := sha256.New()
				for _, rect := range rects {
					x0, x1 := int(rect.X)+1, int(rect.X+rect.W)-1
					for y := int(rect.Y) + 1; y < int(rect.Y+rect.H)-1; y++ {
						start := img.PixOffset(x0, y)
						_, _ = hash.Write(img.Pix[start : start+(x1-x0)*4])
					}
				}
				var pixels [32]byte
				copy(pixels[:], hash.Sum(nil))
				return pixels
			}
			clickButton := func(name string) {
				t.Helper()
				b := button(name).Bounds
				view.ClickAt(b.X+b.W/2, b.Y+b.H/2)
			}

			deniedOtherPixels := regionHash(initial[1], initial[2])
			sendPixels := regionHash(initial[1])
			openDeniedPixels := regionHash(initial[2])

			expectedBounds := initial
			checkStable := func(phase, status string, refreshBusy bool, expectedOtherPixels *[32]byte) {
				t.Helper()
				got := bounds()
				t.Logf("%s: action bounds %v", phase, got)
				if got != expectedBounds {
					t.Errorf("%s moved action buttons: got %v, want %v", phase, got, expectedBounds)
				}
				if !view.HasText(d.tr(status)) {
					t.Errorf("%s lost visible status %q: %v", phase, d.tr(status), view.Texts())
				}
				if node := button(refresh); node.Disabled != refreshBusy || node.Busy != refreshBusy {
					t.Errorf("%s refresh disabled=%v busy=%v, want both %v", phase, node.Disabled, node.Busy, refreshBusy)
				}
				if button(send).Disabled || button(open).Disabled {
					t.Errorf("%s disabled an unrelated action", phase)
				}
				if expectedOtherPixels != nil && regionHash(expectedBounds[1], expectedBounds[2]) != *expectedOtherPixels {
					t.Errorf("%s changed the rendered pixels of an unrelated action", phase)
				}
			}

			// 1. Passive background refresh after TTL leaves the rendered frame byte-identical.
			initialFrame := sha256.Sum256(view.Image().Pix)
			view.Advance(notificationPermissionRefreshInterval)
			view.Frame()
			wait(func() bool { return reads.Load() == 2 })
			view.Advance(200 * time.Millisecond)
			checkStable("background read pending", "未授权", false, &deniedOtherPixels)
			if got := sha256.Sum256(view.Image().Pix); got != initialFrame {
				t.Error("pending background read changed the rendered frame")
			}

			// 2. Manual refresh joins the in-flight passive read and shows delayed feedback in place.
			clickButton(refresh)
			view.Advance(119 * time.Millisecond)
			checkStable("manual refresh joined below delay", "未授权", false, &deniedOtherPixels)
			view.Advance(time.Millisecond)
			checkStable("manual refresh joined after delay", "未授权", true, &deniedOtherPixels)
			complete(result{status: "denied"})
			checkStable("unchanged result", "未授权", false, &deniedOtherPixels)

			// 3. Dedicated manual refresh: repeat activation is absorbed; busy appears only at 120ms.
			clickButton(refresh)
			wait(func() bool { return reads.Load() == 3 })
			view.Advance(119 * time.Millisecond)
			checkStable("manual read below delay", "未授权", false, &deniedOtherPixels)
			clickButton(refresh)
			view.Advance(time.Millisecond)
			checkStable("slow manual read", "未授权", true, &deniedOtherPixels)

			// 4. Transition denied -> granted keeps Refresh and Send bounds fixed, keeps Open aligned
			//    on its right edge/row while calming from Primary to Standard (adding its 1px border).
			complete(result{status: "granted"})
			grantedBounds := bounds()
			if grantedBounds[0] != initial[0] || grantedBounds[1] != initial[1] {
				t.Errorf("changed result moved refresh/send buttons: got %v, want %v", grantedBounds[:2], initial[:2])
			}
			if grantedBounds[2].Y != initial[2].Y || grantedBounds[2].H != initial[2].H ||
				(grantedBounds[2].X+grantedBounds[2].W) != (initial[2].X+initial[2].W) {
				t.Errorf("changed result shifted open-settings anchor: got %v, want same Y/H/right edge as %v", grantedBounds[2], initial[2])
			}
			expectedBounds = grantedBounds
			checkStable("changed result", "已授权", false, nil)
			if regionHash(grantedBounds[1]) != sendPixels {
				t.Error("changed result unexpectedly altered Send Test Notification pixels")
			}
			openGrantedPixels := regionHash(grantedBounds[2])
			if openGrantedPixels == openDeniedPixels {
				t.Error("Open System Settings stayed Primary after permission became granted")
			}
			grantedOtherPixels := regionHash(grantedBounds[1], grantedBounds[2])

			// 5. Failed refresh retains the last known permission and keeps unrelated action pixels unchanged.
			clickButton(refresh)
			wait(func() bool { return reads.Load() == 4 })
			complete(result{err: errors.New("fixture permission read failed")})
			checkStable("failed refresh", "已授权", false, &grantedOtherPixels)
			if !view.HasText(d.tr("通知权限状态读取失败，请重试。")) {
				t.Error("failed refresh did not show retry guidance")
			}

			// 6. Open System Settings & Send Test Notification obey the 120ms busy threshold with fixed bounds.
			clickButton(open)
			wait(func() bool { return openCalls.Load() == 1 })
			view.Advance(119 * time.Millisecond)
			if node := button(open); node.Disabled || node.Busy {
				t.Errorf("open-settings flashed disabled=%v busy=%v below 120ms", node.Disabled, node.Busy)
			}
			clickButton(open) // Absorbed by immediate re-entry guard.
			view.Advance(time.Millisecond)
			if node := button(open); !node.Disabled || !node.Busy || bounds() != expectedBounds {
				t.Errorf("slow open-settings busy/bounds mismatch: disabled=%v busy=%v bounds=%v", node.Disabled, node.Busy, bounds())
			}
			openGate <- nil
			drain()
			if node := button(open); node.Disabled || node.Busy || openCalls.Load() != 1 {
				t.Errorf("open-settings did not settle cleanly: disabled=%v busy=%v calls=%d", node.Disabled, node.Busy, openCalls.Load())
			}

			clickButton(send)
			wait(func() bool { return sendCalls.Load() == 1 })
			view.Advance(119 * time.Millisecond)
			if node := button(send); node.Disabled || node.Busy {
				t.Errorf("send-test flashed disabled=%v busy=%v below 120ms", node.Disabled, node.Busy)
			}
			clickButton(send) // Absorbed by immediate re-entry guard.
			view.Advance(time.Millisecond)
			if node := button(send); !node.Disabled || !node.Busy || bounds() != expectedBounds {
				t.Errorf("slow send-test busy/bounds mismatch: disabled=%v busy=%v bounds=%v", node.Disabled, node.Busy, bounds())
			}
			sendGate <- nil
			drain()
			// Sending a test notification schedules an immediate passive re-read.
			wait(func() bool { return reads.Load() == 5 })
			complete(result{status: "unsupported"})
			if got := bounds(); got != expectedBounds {
				t.Errorf("unsupported result moved action buttons: got %v, want %v", got, expectedBounds)
			}
			if !view.HasText(d.tr("当前环境不支持")) || !button(send).Disabled || !button(open).Disabled {
				t.Error("unsupported result must show its status and disable its unavailable actions")
			}
			if reads.Load() != 5 || openCalls.Load() != 1 || sendCalls.Load() != 1 {
				t.Errorf("host calls: %d reads, %d open, %d send", reads.Load(), openCalls.Load(), sendCalls.Load())
			}
		})
	}
}
