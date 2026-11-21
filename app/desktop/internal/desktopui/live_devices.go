package desktopui

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func (d *viewState) liveDeviceDetail(c *ui.Context, k desktopkit.Kit, v *device) {
	state, tone := deviceStatus(*v)
	state = d.tr(state)
	k.EntityHeader(c, desktopkit.EntityHeaderProps{Icon: v.icon, Title: v.name, Centered: true, Status: desktopkit.EntityStatus{Text: state, Tone: tone}}, nil)
	k.SettingsGroup(c, desktopkit.SettingsGroupProps{Title: d.tr("设备")}, func() {
		k.SettingsRow(c, desktopkit.SettingsRowProps{Label: d.tr("名称")}, nil, func() { k.Text(c, v.name, secondary) })
		if v.id != "local" {
			k.SettingsRow(c, desktopkit.SettingsRowProps{Label: d.tr("最近活动")}, nil, func() { k.Text(c, d.tr(v.lastSeen), secondary) })
		}
	})
	if d.live.message != "" {
		k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneWarning, Title: d.tr(d.live.message)})
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
func (d *viewState) liveDeviceTaskView(c *ui.Context, k desktopkit.Kit) {
	l := d.live
	if l.message != "" {
		k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneWarning, Title: d.tr(l.message)})
	}
	if (d.deviceTask == "invite" || d.deviceTask == "wait") && len(d.pending) > 0 {
		d.approvalTask(c, k, d.pending[0])
		return
	}
	ui.Column(c.Key("live-add-body")).Gap(14).Children(func() {
		if d.deviceTask == "invite" || d.deviceTask == "connect" {
			tab := d.deviceTask
			if k.SegmentedControl(c.Key("add-tab"), desktopkit.SegmentedControlProps{
				Label: d.tr("添加设备"),
				Value: &tab,
				Items: []desktopkit.SegmentedItem{
					{Value: "invite", Label: d.tr("出示配对码")},
					{Value: "connect", Label: d.tr("输入配对码")},
				},
			}).Changed {
				d.setDeviceTask(tab)
			}
		}
		switch d.deviceTask {
		case "invite":
			ticket := d.inviteMode == "ticket"
			k.Text(c, d.tr("在另一台设备的 UniClipboard 中输入下方一次性配对码，随后核对指纹即可完成关联。"), secondary)
			if !ticket {
				d.pairCodeBoxes(c, k, d.inviteCode)
				remaining := max(int(time.Until(time.UnixMilli(l.code.ExpiresAtMs)).Seconds()), 0)
				ui.Row(c.Key("meta")).AlignItems(ui.Center).Gap(6).Children(func() {
					ui.Box(c.Key("dot")).Size(7, 7).Radius(3.5).Background(k.Color("accent-500")).Shrink(0)
					k.Text(c, fmt.Sprintf(d.tr("%d 秒内有效，仅可使用一次。"), remaining), footnote).Grow(1)
					if k.Button(c.Key("copy-code"), d.tr("复制"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy, Disabled: d.inviteCode == ""}).Clicked() {
						mygo.Clipboard.WriteText(d.inviteCode)
					}
					if k.Button(c.Key("regen"), d.tr("刷新"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconRetry, Disabled: l.busy}).Clicked() {
						d.newLiveCode()
					}
				})
			} else {
				ui.Column(c.Key("ticket-box")).Padding(12, 14).Radius(12).Gap(10).
					Background(k.Color("background-secondary-default")).
					Border(1, k.Color("separator-border")).Children(func() {
					if l.ticket != "" {
						k.Text(c, l.ticket, desktopkit.TextProps{Monospace: true, Truncate: true})
					}
					ui.Row(c.Key("ticket-act")).Justify(ui.End).Children(func() {
						if k.Button(c.Key("copy-ticket"), d.tr("复制票据"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy, Disabled: l.ticket == ""}).Clicked() {
							mygo.Clipboard.WriteText(l.ticket)
						}
					})
				})
			}
			ui.Row(c.Key("ticket-toggle")).Justify(ui.Center).Children(func() {
				toggleLabel := d.tr("跨网或命令行配对？改用完整票据 →")
				if ticket {
					toggleLabel = d.tr("← 返回 6 位配对码")
				}
				if k.Button(c.Key("toggle-mode"), toggleLabel, desktopkit.ButtonProps{Variant: desktopkit.Link, Size: desktopkit.SizeSmall}).Clicked() {
					if ticket {
						d.inviteMode = "code"
					} else {
						d.inviteMode = "ticket"
					}
				}
			})
		case "connect":
			kind := connectKind(d.connectInput)
			k.Text(c, d.tr("输入对方提供的配对码或完整票据。"), secondary)
			r := k.FormField(c, desktopkit.FormFieldProps{Label: d.tr("配对码或票据"), Value: &d.connectInput, Placeholder: d.tr("482-913 或 unic-pair:…"), Hint: d.tr("支持 6 位配对码或完整票据。"), AutoFocus: true})
			if (r.Submitted && kind != "" && !l.busy) || k.Button(c.Key("go"), d.tr("连接设备"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: kind == "" || l.busy}).FillWidth().Clicked() {
				d.pairLive(strings.TrimSpace(d.connectInput), "")
			}
		case "wait":
			bg := k.Color("accent-50")
			edge := k.Color("accent-200")
			if k.Dark {
				bg = k.Color("accent-950")
				edge = k.Color("accent-800")
			}
			ui.Row(c.Key("banner")).Padding(12, 14).Radius(12).Gap(10).AlignItems(ui.Center).Background(bg).Border(1, edge).Children(func() {
				k.Icon(c, desktopkit.IconShield, 16).TextColor(k.Color("accent-500")).Shrink(0)
				ui.Column(c.Key("txt")).Gap(2).Grow(1).MinWidth(0).Children(func() {
					k.Text(c, d.tr("等待双方批准"), desktopkit.TextProps{Emphasized: true})
					k.Text(c, d.tr("在另一台设备核对指纹。批准本机收到的请求后，设备才会关联。"), footnote)
				})
			})
			if l.join != nil {
				d.fingerprintGrid(c, k, l.join.Fingerprint)
			}
			ui.Row(c.Key("wait-foot")).Children(func() {
				if k.Button(c.Key("back"), d.tr("返回"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconBack}).Clicked() {
					d.setDeviceTask("connect")
				}
			})
		}
	})
}

// approvalTask shows the fingerprint approval inside the pairing task, so the user need not leave it.
func (d *viewState) approvalTask(c *ui.Context, k desktopkit.Kit, p pendingDevice) {
	l := d.live
	ui.Column(c.Key("live-approval")).Gap(14).Children(func() {
		bg := k.Color("accent-50")
		edge := k.Color("accent-200")
		if k.Dark {
			bg = k.Color("accent-950")
			edge = k.Color("accent-800")
		}
		ui.Row(c.Key("banner")).Padding(12, 14).Radius(12).Gap(10).AlignItems(ui.Center).Background(bg).Border(1, edge).Children(func() {
			k.Icon(c, desktopkit.IconShield, 16).TextColor(k.Color("accent-500")).Shrink(0)
			ui.Column(c.Key("txt")).Gap(2).Grow(1).MinWidth(0).Children(func() {
				k.Text(c, d.tr("核对并批准"), desktopkit.TextProps{Emphasized: true})
				k.Text(c, p.name+" · "+d.tr("对方屏幕上显示相同的指纹时，才接受。"), footnote)
			})
		})
		d.fingerprintGrid(c, k, p.fingerprint)
		k.Text(c, d.tr("名称由对方自报，未经验证。"), footnote)
		ui.Row(c.Key("acts")).Gap(8).Justify(ui.End).Children(func() {
			if k.Button(c.Key("deny"), d.tr("拒绝"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Size: desktopkit.SizeSmall, Disabled: l.busy}).Clicked() {
				d.approveLive(p.id, false)
				d.setDeviceTask("")
				return
			}
			if k.Button(c.Key("accept"), d.tr("接受"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall, Disabled: l.busy}).Clicked() {
				d.approveLive(p.id, true)
			}
		})
	})
}
