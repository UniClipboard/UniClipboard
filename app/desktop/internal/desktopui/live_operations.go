package desktopui

import (
	"context"
	"fmt"
	"math"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

const (
	transferRecentBannerWindow = 8 * time.Second
	transferFallbackPollPeriod = 500 * time.Millisecond
	receiveToastMinBytes       = 256 * 1024 // 256 KiB
	toastIDReceivePrefix       = "receive-op:"
)

func (l *liveState) markOperationsDirty() {
	if l.opsChanged == nil {
		return
	}
	select {
	case l.opsChanged <- struct{}{}:
		l.wake()
	default:
	}
}

func (l *liveState) operationForHistory(id string) (engine.Operation, bool) {
	if l == nil || id == "" || l.opsByHistory == nil {
		return engine.Operation{}, false
	}
	op, ok := l.opsByHistory[id]
	return op, ok
}

func (l *liveState) loadOperations(d *viewState) {
	if l == nil || l.opsLoading || l.client == nil {
		return
	}
	now := time.Now()
	if !l.opsDirty && (l.opsNextRefresh.IsZero() || now.Before(l.opsNextRefresh)) {
		return
	}
	l.opsDirty = false
	l.opsLoading = true
	go func() {
		ctx, cancel := context.WithTimeout(l.ctx, 5*time.Second)
		ops, err := l.client.Operations(ctx, api.OperationsFilter{})
		cancel()
		apply := func() {
			l.opsLoading = false
			if err != nil {
				if l.opsNextRefresh.IsZero() || time.Now().After(l.opsNextRefresh) {
					l.opsNextRefresh = time.Now().Add(2 * time.Second)
				}
				return
			}
			l.applyOperations(ops)
		}
		select {
		case <-l.ctx.Done():
		case l.results <- apply:
			l.wake()
		}
	}()
}

func shouldShowReceiveToast(r *engine.ReceiveOperation) bool {
	if r == nil || r.State != engine.ReceiveReceiving {
		return false
	}
	if r.FileCount > 0 || r.ContentKind == "file" || r.ContentKind == "directory" || r.ContentKind == "mixed" {
		return true
	}
	return r.Progress.TotalBytes >= receiveToastMinBytes
}

func (l *liveState) applyOperations(ops []engine.Operation) {
	now := time.Now()
	byHistory := make(map[string]engine.Operation, len(ops))
	active := make([]engine.Operation, 0, len(ops))
	receives := make([]engine.Operation, 0, 2)
	seenReceives := make(map[string]struct{}, 2)
	hasInFlight := false
	var nextExpiry time.Time

	for _, op := range ops {
		switch op.Kind {
		case engine.OperationSend:
			if op.HistoryID != "" {
				if _, exists := byHistory[string(op.HistoryID)]; !exists {
					byHistory[string(op.HistoryID)] = op
				}
			}
			if !op.Done() {
				hasInFlight = true
				active = append(active, op)
				continue
			}
			age := now.Sub(time.UnixMilli(op.UpdatedAtMs))
			if age >= 0 && age <= transferRecentBannerWindow {
				active = append(active, op)
				exp := time.UnixMilli(op.UpdatedAtMs).Add(transferRecentBannerWindow)
				if nextExpiry.IsZero() || exp.Before(nextExpiry) {
					nextExpiry = exp
				}
			}
		case engine.OperationReceive:
			id := string(op.ID)
			if !op.Done() {
				hasInFlight = true
				seenReceives[id] = struct{}{}
				if _, locallyDiscarded := l.discardedReceives[id]; !locallyDiscarded && shouldShowReceiveToast(op.Receive) {
					receives = append(receives, op)
				}
			}
		}
	}
	for id := range l.discardedReceives {
		if _, stillActive := seenReceives[id]; !stillActive {
			delete(l.discardedReceives, id)
		}
	}
	l.opsByHistory = byHistory
	l.activeOps = active
	l.activeReceives = receives
	switch {
	case hasInFlight:
		l.opsNextRefresh = now.Add(transferFallbackPollPeriod)
	case !nextExpiry.IsZero():
		l.opsNextRefresh = nextExpiry
	default:
		l.opsNextRefresh = time.Time{}
	}
}

func (d *viewState) peerDisplayName(peerID engine.DeviceID, backendID engine.PluginID) string {
	id := string(peerID)
	for _, dev := range d.devices {
		if dev.id == id || dev.deviceID == id {
			if dev.name != "" {
				return dev.name
			}
		}
	}
	if d.live != nil {
		if p, ok := d.live.peers[id]; ok && p.Name != "" {
			return p.Name
		}
	}
	if id != "" {
		return id
	}
	return string(backendID)
}

func (d *viewState) discardReceive(opID string) {
	if d.live == nil || opID == "" {
		return
	}
	l := d.live
	if l.discardedReceives == nil {
		l.discardedReceives = map[string]struct{}{}
	}
	l.discardedReceives[opID] = struct{}{}
	kept := l.activeReceives[:0]
	for _, op := range l.activeReceives {
		if string(op.ID) != opID {
			kept = append(kept, op)
		}
	}
	l.activeReceives = kept
	client := l.client
	if client == nil {
		return
	}
	go func() {
		ctx, cancel := context.WithTimeout(l.ctx, 5*time.Second)
		_ = client.DiscardReceive(ctx, opID)
		cancel()
		l.markOperationsDirty()
	}()
}

func (d *viewState) receiveToastTitle(r *engine.ReceiveOperation) string {
	peer := d.peerDisplayName(r.PeerID, r.BackendID)
	switch {
	case r.ContentKind == "directory":
		return fmt.Sprintf(d.tr("正在接收来自 %s 的文件夹"), peer)
	case r.FileCount > 1:
		return fmt.Sprintf(d.tr("正在接收 %d 个文件（来自 %s）"), r.FileCount, peer)
	case r.FileCount == 1 || r.ContentKind == "file":
		return fmt.Sprintf(d.tr("正在接收来自 %s 的文件"), peer)
	case r.ContentKind == "image":
		return fmt.Sprintf(d.tr("正在接收来自 %s 的图片"), peer)
	default:
		return fmt.Sprintf(d.tr("正在接收来自 %s 的内容"), peer)
	}
}

func (d *viewState) syncReceiveToasts() {
	l := d.live
	activeIDs := map[string]struct{}{}
	if l != nil {
		for _, op := range l.activeReceives {
			if op.Receive == nil {
				continue
			}
			opID := string(op.ID)
			toastID := toastIDReceivePrefix + opID
			activeIDs[toastID] = struct{}{}
			if d.receiveToastIDs == nil {
				d.receiveToastIDs = map[string]struct{}{}
			}
			d.receiveToastIDs[toastID] = struct{}{}

			var progress *float32
			desc := d.tr("正在接收…")
			if op.Receive.Progress.TotalBytes > 0 {
				frac := float32(math.Max(0, math.Min(1, float64(op.Receive.Progress.BytesReceived)/float64(op.Receive.Progress.TotalBytes))))
				progress = &frac
				pct := int(math.Round(float64(frac) * 100))
				desc = fmt.Sprintf("%d%% · %s / %s", pct, humanBytes(op.Receive.Progress.BytesReceived), humanBytes(op.Receive.Progress.TotalBytes))
			}
			d.toasts.SyncPersistent(toastID, d.receiveToastTitle(op.Receive), desktopkit.ToastOptions{
				Status:      desktopkit.ToastInformation,
				Loading:     true,
				Description: desc,
				Progress:    progress,
				CloseLabel:  d.tr("放弃接收"),
				OnDismiss: func() {
					if d.suppressReceiveToastDiscard {
						return
					}
					d.discardReceive(opID)
				},
			})
		}
	}
	for toastID := range d.receiveToastIDs {
		if _, ok := activeIDs[toastID]; ok {
			continue
		}
		d.suppressReceiveToastDiscard = true
		d.toasts.Dismiss(toastID)
		d.suppressReceiveToastDiscard = false
		delete(d.receiveToastIDs, toastID)
	}
}

func (d *viewState) targetDisplayName(t engine.TargetStatus) string {
	id := t.TargetID
	if id == "" {
		id = string(t.PeerID)
	}
	for _, dev := range d.devices {
		if dev.id == id || dev.deviceID == id {
			if dev.name != "" {
				return dev.name
			}
		}
	}
	if d.live != nil {
		if p, ok := d.live.peers[id]; ok && p.Name != "" {
			return p.Name
		}
	}
	if id != "" {
		return id
	}
	return string(t.BackendID)
}

func targetErrorText(d *viewState, err *engine.Error) string {
	if err == nil {
		return ""
	}
	switch err.Code {
	case engine.CodeTimeout:
		return d.tr("连接超时")
	case engine.CodeCancelled:
		return d.tr("已取消")
	case engine.CodeSyncDisabled:
		return d.tr("同步已暂停")
	case engine.CodeSourceChanged:
		return d.tr("源文件已变更或不存在")
	case engine.CodeLimitExceeded:
		return d.tr("超出大小限制")
	default:
		return string(err.Code)
	}
}

func (d *viewState) transferTargetItem(idPrefix string, t engine.TargetStatus) desktopkit.TransferStatusItem {
	item := desktopkit.TransferStatusItem{
		ID:    idPrefix + ":" + string(t.BackendID) + ":" + t.TargetID,
		Label: d.targetDisplayName(t),
	}
	formatBytes := func(p *engine.TransferProgress) string {
		if p == nil || p.TotalBytes <= 0 {
			return ""
		}
		return humanBytes(p.BytesSent) + " / " + humanBytes(p.TotalBytes)
	}
	switch t.State {
	case engine.TargetQueued:
		item.Tone = desktopkit.StatusNeutral
		item.Status = d.tr("排队中")
	case engine.TargetSending:
		item.Tone = desktopkit.StatusInfo
		if t.ReceiptStage == engine.ReceiptStageLocalWritten {
			item.Status = d.tr("等待对端确认…")
			item.Bytes = formatBytes(t.Progress)
		} else if t.Progress != nil && t.Progress.TotalBytes > 0 {
			pct := math.Max(0, math.Min(100, float64(t.Progress.BytesSent)*100/float64(t.Progress.TotalBytes)))
			item.Value = &pct
			item.Status = fmt.Sprintf("%d%%", int(math.Round(pct)))
			item.Bytes = formatBytes(t.Progress)
		} else {
			item.Status = d.tr("正在传输")
		}
	case engine.TargetSent:
		item.Tone = desktopkit.StatusPositive
		switch t.ReceiptStage {
		case engine.ReceiptStageRemoteReceived:
			item.Status = d.tr("对端已接收")
		case engine.ReceiptStageRelayStored:
			item.Status = d.tr("已暂存中转")
		default:
			item.Status = d.tr("已发送")
		}
	case engine.TargetDiscarded:
		item.Tone = desktopkit.StatusWarning
		item.Status = d.tr("对端未接纳")
	case engine.TargetSuperseded:
		item.Tone = desktopkit.StatusNeutral
		item.Status = d.tr("已被新内容替代")
	case engine.TargetFailed:
		item.Tone = desktopkit.StatusCritical
		if t.ReceiptStage == engine.ReceiptStageLocalWritten {
			item.Status = d.tr("未收到对端确认")
			item.Reason = d.tr("本地已写出，但未收到对端确认")
		} else {
			item.Status = d.tr("发送失败")
			item.Reason = targetErrorText(d, t.Error)
		}
	default:
		item.Tone = desktopkit.StatusNeutral
		item.Status = string(t.State)
	}
	return item
}

func (d *viewState) toolbarTransferStatus(c *ui.Context, k desktopkit.Kit) {
	if d.live == nil || len(d.live.activeOps) == 0 {
		return
	}
	items := make([]desktopkit.TransferStatusItem, 0, 4)
	var totalBytes, sentBytes int64
	activeCount, receivedCount, failedCount, discardedCount := 0, 0, 0, 0
	for _, op := range d.live.activeOps {
		for _, t := range op.Targets {
			items = append(items, d.transferTargetItem(string(op.ID), t))
			switch t.State {
			case engine.TargetQueued, engine.TargetSending:
				activeCount++
				if t.Progress != nil && t.Progress.TotalBytes > 0 {
					totalBytes += t.Progress.TotalBytes
					sentBytes += t.Progress.BytesSent
				}
			case engine.TargetSent:
				receivedCount++
			case engine.TargetDiscarded:
				discardedCount++
			case engine.TargetFailed:
				failedCount++
			}
		}
	}
	if len(items) == 0 {
		return
	}
	props := desktopkit.TransferStatusProps{
		Label: d.tr("传输进度"),
		Title: d.tr("传输进度"),
		Items: items,
	}
	switch {
	case activeCount > 0:
		props.Tone = desktopkit.StatusInfo
		props.Status = d.tr("正在传输…")
		props.Summary = fmt.Sprintf(d.tr("正在传输到 %d 台设备"), activeCount)
		if totalBytes > 0 {
			pct := math.Max(0, math.Min(100, float64(sentBytes)*100/float64(totalBytes)))
			props.Value = &pct
		}
	case failedCount > 0:
		props.Tone = desktopkit.StatusCritical
		props.Status = d.tr("发送失败")
		props.Summary = fmt.Sprintf(d.tr("已送达 %d/%d 台设备"), receivedCount, len(items))
	case discardedCount > 0 && receivedCount == 0:
		props.Tone = desktopkit.StatusWarning
		props.Status = d.tr("对端未接纳")
	case discardedCount > 0:
		props.Tone = desktopkit.StatusWarning
		props.Status = d.tr("部分发送")
		props.Summary = fmt.Sprintf(d.tr("已送达 %d/%d 台设备"), receivedCount, len(items))
	default:
		props.Tone = desktopkit.StatusPositive
		props.Status = d.tr("对端已接收")
		props.Summary = fmt.Sprintf(d.tr("已送达 %d/%d 台设备"), receivedCount, len(items))
	}
	k.TransferStatus(c.Key("transfer-status"), props)
}

func looksLikePhoneName(name string) bool {
	low := strings.ToLower(name)
	for _, kw := range []string{"iphone", "ipad", "android", "pixel", "galaxy", "phone", "手机"} {
		if strings.Contains(low, kw) {
			return true
		}
	}
	return false
}

func (d *viewState) deliveryTargetMeta(t engine.TargetStatus) (name string, icon desktopkit.IconName, external bool, retryTarget string) {
	id := t.TargetID
	if id == "" {
		id = string(t.PeerID)
	}
	external = t.TargetKind == engine.TargetGroup

	for _, dev := range d.devices {
		if dev.id == id || dev.deviceID == id {
			if dev.name != "" {
				name = dev.name
			}
			icon = dev.icon
			if dev.id != "" && dev.id != "local" {
				retryTarget = dev.id
			}
			break
		}
	}
	for _, ch := range d.channels {
		if ch.id == id {
			external = true
			if name == "" && ch.name != "" {
				name = ch.name
			}
			break
		}
	}
	if d.live != nil {
		if p, ok := d.live.peers[id]; ok {
			if p.Kind == "external" {
				external = true
			}
			if name == "" && p.Name != "" {
				name = p.Name
			}
			if !external && retryTarget == "" && p.ID != "" {
				retryTarget = p.ID
			}
		}
	}
	if name == "" {
		if id != "" {
			name = id
		} else {
			name = string(t.BackendID)
		}
	}
	if external {
		return name, desktopkit.IconLink, true, ""
	}
	if retryTarget == "" {
		if t.PeerID != "" {
			retryTarget = string(t.PeerID)
		} else if t.TargetKind == engine.TargetDevice && t.TargetID != "" {
			retryTarget = t.TargetID
		}
	}
	if icon == "" || icon == desktopkit.IconDesktop {
		if looksLikePhoneName(name) {
			icon = desktopkit.IconPhone
		} else {
			icon = desktopkit.IconDesktop
		}
	}
	return name, icon, false, retryTarget
}

func (d *viewState) deliveryTargetItem(idPrefix string, t engine.TargetStatus) desktopkit.DeliveryTargetItem {
	name, icon, external, retryTarget := d.deliveryTargetMeta(t)
	sendBusy := d.live != nil && d.live.sendCancel != nil
	item := desktopkit.DeliveryTargetItem{
		ID:            idPrefix + ":" + string(t.BackendID) + ":" + t.TargetID,
		Label:         name,
		Icon:          icon,
		External:      external,
		ChannelLabel:  d.tr("渠道"),
		RetryLabel:    d.tr("重试"),
		RetryDisabled: sendBusy,
	}
	formatBytes := func(p *engine.TransferProgress) string {
		if p == nil || p.TotalBytes <= 0 {
			return ""
		}
		return humanBytes(p.BytesSent) + " / " + humanBytes(p.TotalBytes)
	}
	switch t.State {
	case engine.TargetQueued:
		item.Tone = desktopkit.StatusNeutral
		item.Status = d.tr("排队中")
	case engine.TargetSending:
		item.Tone = desktopkit.StatusInfo
		item.Active = true
		if t.ReceiptStage == engine.ReceiptStageLocalWritten {
			item.Status = d.tr("等待对端确认…")
			item.Bytes = formatBytes(t.Progress)
		} else if t.Progress != nil && t.Progress.TotalBytes > 0 {
			pct := math.Max(0, math.Min(100, float64(t.Progress.BytesSent)*100/float64(t.Progress.TotalBytes)))
			item.Value = &pct
			item.Status = fmt.Sprintf("%d%%", int(math.Round(pct)))
			item.Bytes = formatBytes(t.Progress)
		} else {
			item.Status = d.tr("正在传输")
		}
	case engine.TargetSent:
		item.Tone = desktopkit.StatusPositive
		switch t.ReceiptStage {
		case engine.ReceiptStageRemoteReceived:
			item.Confirmed = true
			item.Status = d.tr("对端已接收")
		case engine.ReceiptStageRelayStored:
			item.Status = d.tr("已暂存中转")
		default:
			item.Status = d.tr("已发送")
		}
	case engine.TargetDiscarded:
		item.Tone = desktopkit.StatusWarning
		item.Status = d.tr("对端未接纳")
		if retryTarget != "" {
			item.Retryable = true
			item.RetryTarget = retryTarget
		}
	case engine.TargetSuperseded:
		item.Tone = desktopkit.StatusNeutral
		item.Status = d.tr("已被新内容替代")
	case engine.TargetFailed:
		item.Tone = desktopkit.StatusCritical
		if t.ReceiptStage == engine.ReceiptStageLocalWritten {
			item.Status = d.tr("未收到对端确认")
			item.Reason = d.tr("本地已写出，但未收到对端确认")
		} else {
			item.Status = d.tr("发送失败")
			item.Reason = targetErrorText(d, t.Error)
		}
		if retryTarget != "" {
			item.Retryable = true
			item.RetryTarget = retryTarget
		}
	default:
		item.Tone = desktopkit.StatusNeutral
		item.Status = string(t.State)
	}
	return item
}

func syncStateTone(s string) desktopkit.StatusTone {
	switch s {
	case "sent":
		return desktopkit.StatusPositive
	case "pending":
		return desktopkit.StatusInfo
	case "partial", "interrupted":
		return desktopkit.StatusWarning
	case "failed":
		return desktopkit.StatusCritical
	default:
		return desktopkit.StatusNeutral
	}
}

func (d *viewState) renderDeliverySection(c *ui.Context, k desktopkit.Kit, v *entry, inDrawer bool) {
	ui.Column(c).Gap(6).Children(func() {
		if d.live != nil {
			if op, ok := d.live.operationForHistory(v.id); ok && len(op.Targets) > 0 {
				items := make([]desktopkit.DeliveryTargetItem, 0, len(op.Targets))
				received, active, failed, discarded := 0, 0, 0, 0
				for _, t := range op.Targets {
					items = append(items, d.deliveryTargetItem(string(op.ID), t))
					switch t.State {
					case engine.TargetSent:
						received++
					case engine.TargetQueued, engine.TargetSending:
						active++
					case engine.TargetFailed:
						failed++
					case engine.TargetDiscarded:
						discarded++
					}
				}
				summary := fmt.Sprintf(d.tr("已送达 %d/%d 台设备"), received, len(op.Targets))
				if active > 0 {
					summary = fmt.Sprintf(d.tr("正在传输到 %d 台设备"), active)
				}
				headTone := desktopkit.StatusPositive
				switch {
				case active > 0:
					headTone = desktopkit.StatusInfo
				case failed > 0:
					headTone = desktopkit.StatusCritical
				case discarded > 0:
					headTone = desktopkit.StatusWarning
				}
				res := k.DeliveryCard(c.Key("card"), desktopkit.DeliveryCardProps{
					EntryID:       v.id,
					ListLabel:     d.tr("送达情况"),
					Summary:       summary,
					Tone:          headTone,
					DefaultOpen:   received < len(op.Targets),
					ExpandLabel:   d.tr("明细"),
					CollapseLabel: d.tr("收起"),
					Compact:       inDrawer,
					InDrawer:      inDrawer,
					Items:         items,
				})
				if res.RetryTarget != "" {
					d.sendTo(res.RetryTarget)
				}
				return
			}
			k.DeliveryCard(c.Key("card"), desktopkit.DeliveryCardProps{
				EntryID:        v.id,
				InDrawer:       inDrawer,
				FallbackPrefix: d.tr("同步状态："),
				FallbackValue:  d.tr(syncStateLabel(v.syncState)),
				FallbackTone:   syncStateTone(v.syncState),
			})
			return
		}
		rows := []desktopkit.InsetListRow{}
		for _, peer := range d.devices {
			if peer.id == "local" {
				continue
			}
			rows = append(rows, desktopkit.InsetListRow{ID: peer.id, Title: peer.name, Status: func() { k.Icon(c, desktopkit.IconInfo, 16).TextColor(k.Color("text-secondary")) }})
		}
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			k.Text(c, d.tr("送达"), desktopkit.TextProps{Emphasized: true})
			k.Text(c, fmt.Sprintf(d.tr("0/%d 台设备"), len(rows)), desktopkit.TextProps{Role: desktopkit.RoleCallout, Tone: desktopkit.ToneSecondary})
		})
		k.InsetList(c, desktopkit.InsetListProps{Label: d.tr("送达情况"), Rows: rows})
		k.Text(c, d.tr("本地加密存储 · 设备之间端到端加密"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
	})
}
