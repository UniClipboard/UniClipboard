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
	if peer, ok := d.live.peers[v.id]; ok && peer.Kind != "external" {
		k.SettingsGroup(c.Key("device-bindings"), uikit.SettingsGroupProps{Title: d.tr("同步关联")}, func() {
			if len(peer.Bindings) == 0 {
				k.Text(c, d.tr("没有已批准关联。请重新配对所需同步实例。"), footnote)
			}
			for _, path := range peer.Bindings {
				path := path
				state := d.tr("待批准")
				if path.State == "authorized" {
					state = d.tr("已批准")
				}
				connection := d.tr("未连接")
				if path.Connection == "connected" {
					connection = d.tr("已连接")
				}
				implementation := ""
				for _, backend := range d.live.syncBackends {
					if backend.ID == path.InstanceID {
						implementation = backend.ImplementationID + " · "
					}
				}
				k.SettingsRow(c.Key(path.ID), uikit.SettingsRowProps{Label: path.InstanceID, Description: implementation + state + " · " + connection}, nil, func() {
					on := path.SyncEnabled
					if k.Switch(c.Key("sync"), &on, uikit.SwitchProps{Label: d.tr("同步") + " " + path.InstanceID, Disabled: d.live.busy || path.State != "authorized"}).Changed() {
						d.mutate(func(ctx context.Context) error {
							_, err := d.live.client.SetBindingSync(ctx, path.ID, api.BindingEdit{Revision: path.Revision, Enabled: on})
							return err
						}, "关联设置已保存")
					}
					if k.Button(c.Key("remove"), d.tr("移除关联"), uikit.ButtonProps{Variant: uikit.Destructive, Disabled: d.live.busy}).Clicked() {
						d.mutate(func(ctx context.Context) error { return d.live.client.RemoveBinding(ctx, path.ID, path.Revision) }, "关联已移除")
					}
				})
			}
		})
	}
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
		peer, err := client.Approve(ctx, id, approval)
		if err == nil {
			// The approved device ends the pairing task.
			select {
			case d.live.results <- func() {
				if d.deviceTask == "invite" || d.deviceTask == "wait" {
					d.deviceTask = ""
					d.deviceList.Value, d.deviceDetailOpen = peer.ID, false
				}
			}:
				d.live.wake()
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return err
	}, "请求已处理")
}
func (d *viewState) pairLive(input string, backend string) {
	client := d.live.client
	localBackend := d.live.pairingBackend
	d.mutate(func(ctx context.Context) error {
		var result api.PairResult
		var err error
		switch {
		case backend != "":
			result, err = client.PairCandidate(ctx, input, backend)
		case connectKind(input) == "配对码":
			result, err = client.JoinVia(ctx, input, localBackend)
		default:
			result, err = client.PairVia(ctx, input, localBackend)
		}
		if err == nil {
			select {
			case d.live.results <- func() { d.live.join = &result; d.deviceTask = "wait" }:
				d.live.wake()
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
	if (d.deviceTask == "invite" || d.deviceTask == "wait") && len(d.pending) > 0 {
		d.approvalTask(c, k, d.pending[0])
		return
	}
	switch d.deviceTask {
	case "invite":
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: d.tr("邀请设备"), Description: d.tr("在另一台设备输入配对码。双方核对指纹后批准。"), BackLabel: d.tr("返回设备列表")}, func() {
			d.pairingBackendSelector(c, k, true)
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
			d.pairingBackendSelector(c, k, false)
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

// approvalTask shows the fingerprint approval inside the pairing task, so the user need not leave it.
func (d *viewState) approvalTask(c *ui.Context, k uikit.Kit, p pendingDevice) {
	l := d.live
	if k.TaskPanel(c, uikit.TaskPanelProps{Title: d.tr("核对并批准"), Description: d.tr("对方屏幕上显示相同的指纹时，才接受。"), BackLabel: d.tr("返回设备列表")}, func() {
		k.Text(c, p.name, uikit.TextProps{Emphasized: true})
		for _, path := range l.pending[p.id].Paths {
			if path.State == "pending" {
				k.Text(c, d.tr("同步实例：")+path.InstanceID, secondary)
			}
		}
		for _, line := range fingerprintLines(p.fingerprint) {
			k.Text(c, line, uikit.TextProps{Monospace: true, Emphasized: true})
		}
		k.Text(c, d.tr("名称由对方自报，未经验证。"), footnote)
	}, func() {
		if k.Button(c.Key("deny"), d.tr("拒绝"), uikit.ButtonProps{Variant: uikit.Destructive, Disabled: l.busy}).Clicked() {
			d.approveLive(p.id, false)
			d.setDeviceTask("")
			return
		}
		if k.Button(c.Key("accept"), d.tr("接受"), uikit.ButtonProps{Variant: uikit.Primary, Disabled: l.busy}).Clicked() {
			d.approveLive(p.id, true)
		}
	}) {
		d.setDeviceTask("")
	}
}

func (d *viewState) pairingBackendSelector(c *ui.Context, k uikit.Kit, invitation bool) {
	l := d.live
	var items []uikit.SelectItem
	for _, b := range l.syncBackends {
		if b.Pairing && b.State == "running" {
			items = append(items, uikit.SelectItem{Value: b.ID, Label: b.ID})
		}
	}
	if len(items) == 0 {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: d.tr("没有可配对的同步后端。请在插件设置中启用支持设备配对的后端并重启。")})
	} else {
		selected := l.pairingBackend
		if k.Select(c.Key("pairing-backend"), uikit.SelectProps{Label: d.tr("同步后端"), Value: &selected, Items: items, Disabled: l.busy}).Changed {
			if invitation {
				d.cancelLiveInvitation()
			}
			l.pairingBackend = selected
			l.codeAttempted, l.ticketAttempted = false, false
		}
	}
}
