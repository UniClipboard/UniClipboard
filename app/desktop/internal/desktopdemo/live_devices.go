package desktopdemo

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func (d *demo) liveDeviceDetail(c *ui.Context, k uikit.Kit, v *device) {
	state, tone := deviceStatus(*v)
	k.EntityHeader(c, uikit.EntityHeaderProps{Icon: v.icon, Title: v.name, Centered: true, Status: uikit.EntityStatus{Text: state, Tone: tone}}, nil)
	k.SettingsGroup(c, uikit.SettingsGroupProps{Title: "设备"}, func() {
		k.SettingsRow(c, uikit.SettingsRowProps{Label: "名称"}, nil, func() { k.Text(c, v.name, secondary) })
		if v.id != "local" {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "最近活动"}, nil, func() { k.Text(c, v.lastSeen, secondary) })
		}
	})
	if d.live.message != "" {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: d.live.message})
	}
}
func (d *demo) approveLive(id string, accept bool) {
	p, ok := d.live.pending[id]
	if !ok {
		return
	}
	client := d.live.client
	// Preserve the exact displayed approval snapshot across the asynchronous call.
	approval := api.Approval{Fingerprint: p.Fingerprint, Revision: p.Revision, Bindings: append([]api.BindingApproval{}, p.Bindings...)}
	d.mutate(func(ctx context.Context) error {
		if !accept {
			return client.Reject(ctx, id)
		}
		_, err := client.Approve(ctx, id, approval)
		return err
	}, "请求已处理")
}
func (d *demo) pairLive(input string, backend string) {
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		var result api.PairResult
		var err error
		switch {
		case backend != "":
			result, err = client.PairCandidate(ctx, input, backend)
		case connectKind(input) == "配对码":
			result, err = client.Join(ctx, input)
		default:
			result, err = client.Pair(ctx, input)
		}
		if err == nil {
			select {
			case d.live.results <- func() { d.live.join = &result; d.deviceTask = "wait" }:
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return err
	}, "请求已发送，请核对指纹")
}
func (d *demo) cancelLiveInvitation() {
	l := d.live
	l.ticket = ""
	l.code = api.Code{}
	d.inviteCode = ""
	l.enqueue(func(ctx context.Context) func() {
		_ = l.client.CancelCode(ctx)
		return func() { l.code = api.Code{}; l.ticket = ""; d.inviteCode = "" }
	})
}
func (d *demo) liveDeviceTaskView(c *ui.Context, k uikit.Kit) {
	l := d.live
	if l.message != "" {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: l.message})
	}
	switch d.deviceTask {
	case "invite":
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: "邀请设备", Description: "在另一台设备输入配对码。双方核对指纹后批准。", BackLabel: "返回设备列表"}, func() {
			mode := d.inviteMode
			if k.SegmentedControl(c, uikit.SegmentedControlProps{Label: "邀请方式", Value: &mode, Items: []uikit.SegmentedItem{{Value: "code", Label: "配对码"}, {Value: "ticket", Label: "完整票据"}}}).Changed {
				d.inviteMode = mode
			}
			if d.inviteMode == "ticket" {
				if l.ticket != "" {
					k.Text(c, l.ticket, uikit.TextProps{Monospace: true, Truncate: true})
				}
				if k.Button(c.Key("ticket"), "生成票据", uikit.ButtonProps{Disabled: l.busy}).Clicked() {
					client := l.client
					d.mutate(func(ctx context.Context) error {
						ticket, err := client.ExportTicket(ctx)
						if err == nil {
							select {
							case l.results <- func() { l.ticket = ticket }:
							case <-ctx.Done():
								return ctx.Err()
							}
						}
						return err
					}, "票据已生成")
				}
				if k.Button(c.Key("copy-ticket"), "复制票据", uikit.ButtonProps{Disabled: l.ticket == ""}).Clicked() {
					mygo.Clipboard.WriteText(l.ticket)
				}
			} else {
				k.Text(c, d.inviteCode, uikit.TextProps{Large: true, Monospace: true, Emphasized: true})
				remaining := max(int(time.Until(time.UnixMilli(l.code.ExpiresAtMs)).Seconds()), 0)
				k.Text(c, fmt.Sprintf("%d 秒内有效，仅可使用一次。", remaining), footnote)
			}
		}, func() {
			if k.Button(c.Key("regen"), "重新生成", uikit.ButtonProps{Disabled: l.busy}).Clicked() {
				d.newLiveCode()
			}
		}) {
			d.setDeviceTask("")
		}
	case "connect":
		kind := connectKind(d.connectInput)
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: "连接设备", Description: "输入对方提供的配对码或完整票据。", BackLabel: "返回设备列表"}, func() {
			k.FormField(c, uikit.FormFieldProps{Label: "配对码或票据", Value: &d.connectInput, Placeholder: "482-913 或 unic-pair:…", Hint: "支持 6 位配对码或完整票据。"})
		}, func() {
			if k.Button(c.Key("go"), "连接", uikit.ButtonProps{Variant: uikit.Primary, Disabled: kind == "" || l.busy}).Clicked() {
				d.pairLive(strings.TrimSpace(d.connectInput), "")
			}
		}) {
			d.setDeviceTask("")
		}
	case "wait":
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: "等待双方批准", Description: "在另一台设备核对指纹。批准本机收到的请求后，设备才会关联。", BackLabel: "返回设备列表"}, func() {
			if l.join != nil {
				k.Text(c, l.join.Fingerprint, uikit.TextProps{Monospace: true, Emphasized: true})
			}
		}, nil) {
			d.setDeviceTask("")
		}
	case "phone":
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: "连接手机", Description: "在手机端输入本机生成的配对码。", BackLabel: "返回设备列表"}, nil, func() {
			if k.Button(c, "获取配对码", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				d.setDeviceTask("invite")
			}
		}) {
			d.setDeviceTask("")
		}
	}
}
