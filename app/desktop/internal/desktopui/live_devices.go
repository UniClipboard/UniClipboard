package desktopui

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

func (d *viewState) liveDeviceDetail(c *ui.Context, k uikit.Kit, v *device) {
	state, tone := deviceStatus(*v)
	state = d.tr(state)
	k.EntityHeader(c, uikit.EntityHeaderProps{Icon: v.icon, Title: v.name, Centered: true, Status: uikit.EntityStatus{Text: state, Tone: tone}}, nil)
	k.SettingsGroup(c, uikit.SettingsGroupProps{Title: d.tr("设备")}, func() {
		k.SettingsRow(c, uikit.SettingsRowProps{Label: d.tr("名称")}, nil, func() { k.Text(c, v.name, secondary) })
		if v.id != "local" {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: d.tr("最近活动")}, nil, func() { k.Text(c, d.tr(v.lastSeen), secondary) })
		}
	})
	if d.live.message != "" {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: d.tr(d.live.message)})
	}
}
func (d *viewState) approveLive(id string, accept bool) {
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
func (d *viewState) pairLive(input string, backend string) {
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
func (d *viewState) cancelLiveInvitation() {
	l := d.live
	l.ticket = ""
	l.code = api.Code{}
	d.inviteCode = ""
	l.enqueue(func(ctx context.Context) func() {
		_ = l.client.CancelCode(ctx)
		return func() { l.code = api.Code{}; l.ticket = ""; d.inviteCode = "" }
	})
}
func (d *viewState) liveDeviceTaskView(c *ui.Context, k uikit.Kit) {
	l := d.live
	if l.message != "" {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: d.tr(l.message)})
	}
	switch d.deviceTask {
	case "invite":
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: d.tr("邀请设备"), Description: d.tr("在另一台设备输入配对码。双方核对指纹后批准。"), BackLabel: d.tr("返回设备列表")}, func() {
			mode := d.inviteMode
			if k.SegmentedControl(c, uikit.SegmentedControlProps{Label: d.tr("邀请方式"), Value: &mode, Items: []uikit.SegmentedItem{{Value: "code", Label: d.tr("配对码")}, {Value: "ticket", Label: d.tr("完整票据")}}}).Changed {
				d.inviteMode = mode
			}
			if d.inviteMode == "ticket" {
				if l.ticket != "" {
					k.Text(c, l.ticket, uikit.TextProps{Monospace: true, Truncate: true})
				}
			} else {
				k.Text(c, d.inviteCode, uikit.TextProps{Large: true, Monospace: true, Emphasized: true})
				remaining := max(int(time.Until(time.UnixMilli(l.code.ExpiresAtMs)).Seconds()), 0)
				k.Text(c, fmt.Sprintf(d.tr("%d 秒内有效，仅可使用一次。"), remaining), footnote)
			}
		}, func() {
			if d.inviteMode == "ticket" {
				if k.Button(c.Key("copy-ticket"), d.tr("复制票据"), uikit.ButtonProps{Icon: uikit.IconCopy, Disabled: l.ticket == ""}).Clicked() {
					mygo.Clipboard.WriteText(l.ticket)
				}
			} else if k.Button(c.Key("regen"), d.tr("重新生成"), uikit.ButtonProps{Disabled: l.busy}).Clicked() {
				d.newLiveCode()
			}
		}) {
			d.setDeviceTask("")
		}
	case "connect":
		kind := connectKind(d.connectInput)
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: d.tr("连接设备"), Description: d.tr("输入对方提供的配对码或完整票据。"), BackLabel: d.tr("返回设备列表")}, func() {
			k.FormField(c, uikit.FormFieldProps{Label: d.tr("配对码或票据"), Value: &d.connectInput, Placeholder: d.tr("482-913 或 unic-pair:…"), Hint: d.tr("支持 6 位配对码或完整票据。")})
		}, func() {
			if k.Button(c.Key("go"), d.tr("连接"), uikit.ButtonProps{Variant: uikit.Primary, Disabled: kind == "" || l.busy}).Clicked() {
				d.pairLive(strings.TrimSpace(d.connectInput), "")
			}
		}) {
			d.setDeviceTask("")
		}
	case "wait":
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: d.tr("等待双方批准"), Description: d.tr("在另一台设备核对指纹。批准本机收到的请求后，设备才会关联。"), BackLabel: d.tr("返回设备列表")}, func() {
			if l.join != nil {
				k.Text(c, l.join.Fingerprint, uikit.TextProps{Monospace: true, Emphasized: true})
			}
		}, nil) {
			d.setDeviceTask("")
		}
	case "phone":
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: d.tr("连接手机"), Description: d.tr("在手机端输入本机生成的配对码。"), BackLabel: d.tr("返回设备列表")}, nil, func() {
			if k.Button(c, d.tr("获取配对码"), uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				d.setDeviceTask("invite")
			}
		}) {
			d.setDeviceTask("")
		}
	}
}
