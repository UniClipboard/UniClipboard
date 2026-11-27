package desktopkit

import (
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestDeliveryCardZeroHeightShiftAcrossStates(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		pct42 := 42.0
		states := []struct {
			name    string
			summary string
			tone    StatusTone
			first   DeliveryTargetItem
		}{
			{
				name:    "failed",
				summary: "已送达 2/3 台设备",
				tone:    StatusCritical,
				first: DeliveryTargetItem{
					ID: "iphone", Label: "iPhone", Icon: IconPhone,
					Status: "未收到对端确认", Tone: StatusCritical,
					Reason:    "本地已写出，但未收到对端确认",
					Retryable: true, RetryLabel: "重试", RetryTarget: "dev-iphone",
				},
			},
			{
				name:    "queued",
				summary: "正在传输到 1 台设备",
				tone:    StatusInfo,
				first: DeliveryTargetItem{
					ID: "iphone", Label: "iPhone", Icon: IconPhone,
					Status: "排队中", Tone: StatusNeutral,
				},
			},
			{
				name:    "sending",
				summary: "正在传输到 1 台设备",
				tone:    StatusInfo,
				first: DeliveryTargetItem{
					ID: "iphone", Label: "iPhone", Icon: IconPhone,
					Status: "42%", Tone: StatusInfo, Active: true, Value: &pct42, Bytes: "42.0 MB / 100.0 MB",
				},
			},
			{
				name:    "awaiting",
				summary: "正在传输到 1 台设备",
				tone:    StatusInfo,
				first: DeliveryTargetItem{
					ID: "iphone", Label: "iPhone", Icon: IconPhone,
					Status: "等待对端确认…", Tone: StatusInfo, Active: true, Bytes: "100.0 MB / 100.0 MB",
				},
			},
			{
				name:    "sent",
				summary: "已送达 3/3 台设备",
				tone:    StatusPositive,
				first: DeliveryTargetItem{
					ID: "iphone", Label: "iPhone", Icon: IconPhone,
					Status: "对端已接收", Tone: StatusPositive, Confirmed: true,
				},
			},
			{
				name:    "discarded",
				summary: "已送达 2/3 台设备",
				tone:    StatusWarning,
				first: DeliveryTargetItem{
					ID: "iphone", Label: "iPhone", Icon: IconPhone,
					Status: "对端未接纳", Tone: StatusWarning,
					Retryable: true, RetryLabel: "重试", RetryTarget: "dev-iphone",
				},
			},
		}

		current := states[0]
		var retried string
		var cardOpen bool
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).FillWidth().Padding(16).Children(func() {
				res := k.DeliveryCard(c.Key("card"), DeliveryCardProps{
					EntryID:       "hist-1",
					ListLabel:     "送达情况",
					Summary:       current.summary,
					Tone:          current.tone,
					DefaultOpen:   true,
					ExpandLabel:   "明细",
					CollapseLabel: "收起",
					Items: []DeliveryTargetItem{
						current.first,
						{ID: "mac", Label: "Mac-mini.local", Icon: IconDesktop, Status: "对端已接收", Tone: StatusPositive, Confirmed: true},
						{ID: "wetype", Label: "WeType", Icon: IconLink, External: true, ChannelLabel: "渠道", Status: "已发送", Tone: StatusPositive},
					},
				})
				if res.RetryTarget != "" {
					retried = res.RetryTarget
				}
				cardOpen = res.Open
				// Anchor element right below DeliveryCard to measure exact vertical position.
				ui.Box(c.Key("anchor")).Height(10).Label("bottom-anchor")
			})
		}, 520, 420)

		tt.Frame()
		baseRect, ok := tt.Find("bottom-anchor")
		if !ok {
			t.Fatal("missing bottom-anchor")
		}
		if !cardOpen {
			t.Fatal("expected card to start open when DefaultOpen=true")
		}
		if err := tt.Click("重试 iPhone"); err != nil {
			t.Fatalf("click retry: %v", err)
		}
		if retried != "dev-iphone" {
			t.Fatalf("retried = %q, want dev-iphone", retried)
		}

		for _, st := range states[1:] {
			current = st
			tt.Frame()
			if !cardOpen {
				t.Fatalf("state %s unexpectedly auto-collapsed card", st.name)
			}
			rect, ok := tt.Find("bottom-anchor")
			if !ok {
				t.Fatalf("state %s missing bottom-anchor", st.name)
			}
			if math.Abs(float64(rect.Y-baseRect.Y)) > 0.01 {
				t.Fatalf("state %s shifted height: anchor Y=%.2f, want %.2f", st.name, rect.Y, baseRect.Y)
			}
		}

		// User can collapse and re-expand via header trigger.
		if err := tt.Click(current.summary); err != nil {
			t.Fatalf("collapse click: %v", err)
		}
		tt.Frame()
		if cardOpen {
			t.Fatal("expected card to collapse after clicking header")
		}
		if tt.HasText("Mac-mini.local") && tt.HasText("对端已接收") {
			t.Fatal("expanded row status should be hidden while collapsed")
		}
	})
}

func TestDeliveryCardCollapsedDefaultAndFallback(t *testing.T) {
	k := Kit{}
	tt := ui.NewTester(func(c *ui.Context) {
		k.Apply(c)
		ui.Column(c).FillWidth().Padding(16).Gap(12).Children(func() {
			k.DeliveryCard(c.Key("healthy"), DeliveryCardProps{
				EntryID:       "hist-ok",
				ListLabel:     "送达情况",
				Summary:       "已送达 2/2 台设备",
				Tone:          StatusPositive,
				DefaultOpen:   false,
				ExpandLabel:   "明细",
				CollapseLabel: "收起",
				Items: []DeliveryTargetItem{
					{ID: "mac", Label: "Mac-mini.local", Icon: IconDesktop, Status: "对端已接收", Tone: StatusPositive, Confirmed: true},
					{ID: "wetype", Label: "WeType", Icon: IconLink, External: true, ChannelLabel: "渠道", Status: "已发送", Tone: StatusPositive},
				},
			})
			k.DeliveryCard(c.Key("fallback"), DeliveryCardProps{
				EntryID:        "hist-old",
				FallbackPrefix: "同步状态：",
				FallbackValue:  "已发送",
				FallbackTone:   StatusPositive,
			})
		})
	}, 520, 260)

	tt.Frame()
	if !tt.HasText("已送达 2/2 台设备") || !tt.HasText("明细") {
		t.Fatalf("expected collapsed summary strip, texts=%v", tt.Texts())
	}
	if tt.HasText("对端已接收") {
		t.Fatalf("expected healthy card to start collapsed, texts=%v", tt.Texts())
	}
	if !tt.HasText("同步状态：") || !tt.HasText("已发送") {
		t.Fatalf("expected fallback strip, texts=%v", tt.Texts())
	}
}
