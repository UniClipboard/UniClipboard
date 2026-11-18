package desktopui

import (
	"fmt"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// deliveryView is the presentation of one device row of the delivery list.
// "sent" only means every byte was written to the connection, so it reads 已发送.
type deliveryView struct {
	status, reason string
	tone           uikit.StatusTone
	sent, retry    bool
}

// deliveryOf maps the stored send result of a device (nil: never sent) to its row.
func (d *viewState) deliveryOf(t *api.HistoryTarget) deliveryView {
	if t == nil {
		return deliveryView{status: d.tr("未发送"), tone: uikit.StatusNeutral, retry: true}
	}
	switch t.State {
	case "queued":
		return deliveryView{status: d.tr("排队中"), tone: uikit.StatusInfo}
	case "sending":
		return deliveryView{status: d.tr("发送中"), tone: uikit.StatusInfo}
	case "sent":
		return deliveryView{status: d.tr("已发送"), tone: uikit.StatusPositive, sent: true}
	case "superseded":
		return deliveryView{status: d.tr("已被新内容替代"), tone: uikit.StatusNeutral, retry: true}
	}
	reason := d.tr("发送未完成")
	switch t.ErrorCode {
	case "interrupted":
		reason = d.tr("应用退出时发送尚未完成")
	case "sync_disabled":
		reason = d.tr("同步已关闭")
	case "unavailable":
		reason = d.tr("连接不可用")
	}
	return deliveryView{status: d.tr("发送失败"), reason: reason, tone: uikit.StatusCritical, retry: true}
}

func historyTarget(v entry, device string) *api.HistoryTarget {
	for i := range v.targets {
		if v.targets[i].TargetKind == "device" && v.targets[i].TargetID == device {
			return &v.targets[i]
		}
	}
	return nil
}

// liveDelivery renders the per-device delivery list of a live history entry.
func (d *viewState) liveDelivery(c *ui.Context, k uikit.Kit, v entry) {
	var peers []device
	for _, p := range d.devices {
		if p.id != "local" {
			peers = append(peers, p)
		}
	}
	busy := d.live.sendCancel != nil
	rows := make([]uikit.InsetListRow, 0, len(peers))
	sent := 0
	for _, p := range peers {
		p, view := p, d.deliveryOf(historyTarget(v, p.id))
		if view.sent {
			sent++
		}
		rows = append(rows, uikit.InsetListRow{ID: p.id, Title: p.name, Status: func() {
			props := uikit.TransferStatusProps{
				Label: p.name + ": " + view.status, Title: p.name, Status: view.status, Tone: view.tone,
				Retryable: view.retry, Disabled: busy,
			}
			if view.reason != "" {
				props.Summary = view.reason
			}
			props.Value, props.Items, props.Summary = d.transferItems(historyTarget(v, p.id), props.Summary)
			if k.TransferStatus(c.Key("transfer-"+p.id), props).Retry {
				d.resendTo = p.id
			}
		}})
	}
	ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
		k.Text(c, d.tr("送达"), uikit.TextProps{Emphasized: true})
		if len(peers) > 0 {
			k.Text(c, fmt.Sprintf(d.tr("%d/%d 台设备"), sent, len(peers)), uikit.TextProps{Role: uikit.RoleCallout, Tone: uikit.ToneSecondary})
		}
	})
	if len(peers) == 0 {
		k.Text(c, d.tr("暂未配对任何设备"), uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
	} else {
		k.InsetList(c, uikit.InsetListProps{Label: d.tr("送达情况"), Rows: rows})
	}
	k.Text(c, d.tr("本地加密存储 · 设备之间端到端加密"), uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneTertiary})
}

// resendDialog asks before sending the entry again to one device: the device may keep
// duplicate files from the earlier attempt.
func (d *viewState) resendDialog(c *ui.Context, k uikit.Kit) {
	if d.resendTo == "" || d.live == nil {
		return
	}
	open := true
	name := d.sourceName(d.resendTo)
	r := k.AlertDialog(c, uikit.AlertDialogProps{Open: &open, Title: fmt.Sprintf(d.tr("重新发送到 %s？"), name),
		Description:  d.tr("之前的发送没有完成或没有结果。对方设备上可能出现重复内容。"),
		ConfirmLabel: d.tr("重新发送"), CancelLabel: d.tr("取消")}, nil)
	if r.Confirmed {
		d.startHistorySend(d.resendTo)
		open = false
	}
	if !open {
		d.resendTo = ""
	}
}

// transferItems turns the live items of a running send into the ring value and the popover
// rows. The total is known only when every part reports one; otherwise the ring spins.
func (d *viewState) transferItems(t *api.HistoryTarget, summary string) (*float64, []uikit.TransferStatusItem, string) {
	if t == nil || len(t.Items) == 0 {
		return nil, nil, summary
	}
	var done, total int64
	known := true
	items := make([]uikit.TransferStatusItem, 0, len(t.Items))
	for _, it := range t.Items {
		done += it.CompletedBytes
		total += it.TotalBytes
		known = known && it.TotalBytes > 0
		row := uikit.TransferStatusItem{ID: fmt.Sprint(it.Index), Label: it.Name,
			Bytes: humanBytes(it.CompletedBytes) + " / " + humanBytes(it.TotalBytes)}
		if row.Label == "" {
			row.Label = fmt.Sprintf(d.tr("项目 %d"), it.Index+1)
		}
		switch it.State {
		case "completed":
			row.Status, row.Tone = d.tr("完成"), uikit.StatusPositive
		case "failed":
			row.Status, row.Tone = d.tr("发送失败"), uikit.StatusCritical
		case "transferring":
			row.Status, row.Tone = d.tr("传输中"), uikit.StatusInfo
			if it.TotalBytes > 0 {
				v := float64(it.CompletedBytes) * 100 / float64(it.TotalBytes)
				row.Value = &v
			}
		default:
			row.Status, row.Tone = d.tr("等待"), uikit.StatusNeutral
		}
		items = append(items, row)
	}
	var value *float64
	if known {
		v := float64(done) * 100 / float64(total)
		value = &v
	}
	return value, items, humanBytes(done) + " / " + humanBytes(total)
}
