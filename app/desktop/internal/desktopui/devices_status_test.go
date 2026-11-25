package desktopui

import (
	"testing"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

func TestDeviceAggregateStatusDetailedStates(t *testing.T) {
	d := &viewState{language: "zh-CN"}

	cases := []struct {
		name       string
		bindings   []deviceBinding
		wantText   string
		wantTone   desktopkit.StatusTone
		wantOnline bool
	}{
		{
			name: "connected direct",
			bindings: []deviceBinding{{
				state: "authorized", syncEnabled: true, connection: "connected", connectionPath: "direct", latencyMs: 4,
			}},
			wantText: "在线 · 直连", wantTone: desktopkit.StatusPositive, wantOnline: true,
		},
		{
			name: "connected derp",
			bindings: []deviceBinding{{
				state: "authorized", syncEnabled: true, connection: "connected", connectionPath: "derp", latencyMs: 42,
			}},
			wantText: "在线 · 中继", wantTone: desktopkit.StatusPositive, wantOnline: true,
		},
		{
			name: "connected unknown path",
			bindings: []deviceBinding{{
				state: "authorized", syncEnabled: true, connection: "connected", connectionPath: "unknown",
			}},
			wantText: "在线", wantTone: desktopkit.StatusPositive, wantOnline: true,
		},
		{
			name: "connecting",
			bindings: []deviceBinding{{
				state: "authorized", syncEnabled: true, connection: "connecting",
			}},
			wantText: "重连中", wantTone: desktopkit.StatusWarning, wantOnline: false,
		},
		{
			name: "disconnected",
			bindings: []deviceBinding{{
				state: "authorized", syncEnabled: true, connection: "disconnected",
			}},
			wantText: "离线", wantTone: desktopkit.StatusNeutral, wantOnline: false,
		},
		{
			name: "paused",
			bindings: []deviceBinding{{
				state: "authorized", syncEnabled: false, connection: "connecting",
			}},
			wantText: "已暂停", wantTone: desktopkit.StatusNeutral, wantOnline: false,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			gotText, gotTone, gotOnline := d.deviceAggregateStatus(device{id: "dev-1", bindings: tc.bindings})
			if gotText != tc.wantText || gotTone != tc.wantTone || gotOnline != tc.wantOnline {
				t.Fatalf("got (%q, %v, %v), want (%q, %v, %v)", gotText, gotTone, gotOnline, tc.wantText, tc.wantTone, tc.wantOnline)
			}
		})
	}
}
