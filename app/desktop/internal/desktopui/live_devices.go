package desktopui

import (
	"context"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func (d *viewState) approveLive(id string, accept bool) {
	if !accept && d.isExistingPendingDevice(id) {
		d.rejectLivePendingPaths(id)
		return
	}
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
					d.deviceTaskForID = ""
					d.live.join = nil
					d.clearPairingQR()
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

// rejectLivePendingPaths removes only pending bindings on an already-trusted device,
// preserving the trusted device record and all existing authorized paths.
func (d *viewState) rejectLivePendingPaths(id string) {
	type target struct {
		id  string
		rev int64
	}
	var targets []target
	if p, ok := d.live.pending[id]; ok {
		for _, path := range p.Paths {
			if path.State == "pending" {
				targets = append(targets, target{id: path.ID, rev: path.Revision})
			}
		}
	}
	if len(targets) == 0 {
		if dev := d.deviceByID(id); dev != nil {
			for _, b := range dev.bindings {
				if b.state == "pending" {
					targets = append(targets, target{id: b.id, rev: b.revision})
				}
			}
		}
	}
	if len(targets) == 0 {
		return
	}
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		for _, t := range targets {
			if err := client.RemoveBinding(ctx, t.id, t.rev); err != nil {
				return err
			}
		}
		return nil
	}, "已拒绝新增路径请求")
}

func (d *viewState) findDeviceBinding(bindingID string) (*device, *deviceBinding) {
	for i := range d.devices {
		for j := range d.devices[i].bindings {
			if d.devices[i].bindings[j].id == bindingID {
				return &d.devices[i], &d.devices[i].bindings[j]
			}
		}
	}
	return nil, nil
}

func (d *viewState) applyLiveBindingUpdate(updated engine.DeviceBinding) {
	if updated.ID == "" {
		return
	}
	_, b := d.findDeviceBinding(updated.ID)
	if b == nil {
		return
	}
	b.syncEnabled = updated.SyncEnabled
	b.policy = updated.Policy.Normalized()
	if updated.Revision > 0 {
		b.revision = updated.Revision
	}
	if updated.State != "" {
		b.state = updated.State
	}
	if updated.Connection != "" {
		b.connection = string(updated.Connection)
	}
	if updated.ConnectionPath != "" {
		b.connectionPath = updated.ConnectionPath
	}
	if updated.LatencyMs > 0 {
		b.latencyMs = updated.LatencyMs
	}
	if updated.LastCheckedMs > 0 {
		b.lastChecked = formatLastActiveMs(updated.LastCheckedMs)
	}
	if updated.LastActiveMs > 0 {
		b.lastActive = formatLastActiveMs(updated.LastActiveMs)
	}
	b.lastError = updated.LastError
}

func (d *viewState) applyLiveChannelUpdate(updated api.PeerStatus) {
	if updated.ID == "" {
		return
	}
	ch := d.channelByID(updated.ID)
	if ch == nil {
		return
	}
	if updated.Name != "" {
		ch.name = updated.Name
	}
	ch.enabled = updated.Enabled
	ch.autoApply = updated.AutoApply
	ch.remoteRevocation = updated.RemoteRevocation
	if updated.Revision > 0 {
		ch.revision = updated.Revision
	}
	if updated.LastSeen > 0 {
		ch.lastSeen = formatSeen(updated.LastSeen)
	}
	ch.lastError = updated.LastError
}

func (d *viewState) applyLivePeerRename(updated api.PeerStatus) {
	if updated.ID == "" {
		return
	}
	dev := d.deviceByID(updated.ID)
	if dev == nil {
		return
	}
	if updated.Name != "" {
		dev.name = updated.Name
	}
	dev.userName = updated.UserName
	if updated.ReportedName != "" {
		dev.reportedName = updated.ReportedName
	}
	if updated.Revision > 0 {
		dev.revision = updated.Revision
	}
}

func (d *viewState) renameLivePeer(id string, rev int64, name string) {
	if d.live.busy {
		return
	}
	client := d.live.client
	msg := "设备名称已保存"
	if name == "" {
		msg = "已恢复为对方自报名"
	}
	d.mutate(func(ctx context.Context) error {
		peer, err := client.RenamePeer(ctx, id, api.PeerRename{Revision: rev, Name: name})
		if err == nil && peer.ID != "" {
			select {
			case d.live.results <- func() { d.applyLivePeerRename(peer) }:
				d.live.wake()
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return err
	}, msg)
}

func (d *viewState) setLiveBindingSync(bindingID string, rev int64, enabled bool) {
	if d.live.busy {
		return
	}
	client := d.live.client
	en := enabled
	hadPrev := false
	prevEnabled := false
	if _, b := d.findDeviceBinding(bindingID); b != nil {
		hadPrev = true
		prevEnabled = b.syncEnabled
		b.syncEnabled = enabled
	}
	d.mutate(func(ctx context.Context) error {
		updated, err := client.SetBindingSync(ctx, bindingID, api.BindingEdit{Revision: rev, Enabled: &en})
		if err == nil {
			if updated.ID != "" {
				select {
				case d.live.results <- func() { d.applyLiveBindingUpdate(updated) }:
					d.live.wake()
				case <-ctx.Done():
					return ctx.Err()
				}
			}
			return nil
		}
		if hadPrev {
			select {
			case d.live.results <- func() {
				if _, b := d.findDeviceBinding(bindingID); b != nil && b.revision == rev {
					b.syncEnabled = prevEnabled
				}
			}:
				d.live.wake()
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return err
	}, "同步路径设置已保存")
	if !d.live.busy && hadPrev {
		if _, b := d.findDeviceBinding(bindingID); b != nil && b.revision == rev {
			b.syncEnabled = prevEnabled
		}
	}
}

func (d *viewState) setLiveBindingPolicy(bindingID string, rev int64, enabled *bool, policy engine.BindingSyncPolicy) {
	if d.live.busy {
		return
	}
	client := d.live.client
	pol := policy.Normalized()
	hadPrev := false
	var prevEnabled bool
	var prevPolicy engine.BindingSyncPolicy
	if _, b := d.findDeviceBinding(bindingID); b != nil {
		hadPrev = true
		prevEnabled = b.syncEnabled
		prevPolicy = b.policy
		b.policy = pol
		if enabled != nil {
			b.syncEnabled = *enabled
		}
	}
	d.mutate(func(ctx context.Context) error {
		updated, err := client.SetBindingSync(ctx, bindingID, api.BindingEdit{Revision: rev, Enabled: enabled, Policy: &pol})
		if err == nil {
			if updated.ID != "" {
				select {
				case d.live.results <- func() { d.applyLiveBindingUpdate(updated) }:
					d.live.wake()
				case <-ctx.Done():
					return ctx.Err()
				}
			}
			return nil
		}
		if hadPrev {
			select {
			case d.live.results <- func() {
				if _, b := d.findDeviceBinding(bindingID); b != nil && b.revision == rev {
					b.syncEnabled = prevEnabled
					b.policy = prevPolicy
				}
			}:
				d.live.wake()
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return err
	}, "同步策略已保存")
	if !d.live.busy && hadPrev {
		if _, b := d.findDeviceBinding(bindingID); b != nil && b.revision == rev {
			b.syncEnabled = prevEnabled
			b.policy = prevPolicy
		}
	}
}

func (d *viewState) setLiveAllBindingsSync(bindings []deviceBinding, enabled bool) {
	if d.live.busy {
		return
	}
	type target struct {
		id          string
		rev         int64
		prevEnabled bool
	}
	var targets []target
	for _, b := range bindings {
		if b.state == "authorized" && b.syncEnabled != enabled {
			targets = append(targets, target{id: b.id, rev: b.revision, prevEnabled: b.syncEnabled})
		}
	}
	if len(targets) == 0 {
		return
	}
	for _, t := range targets {
		if _, b := d.findDeviceBinding(t.id); b != nil {
			b.syncEnabled = enabled
		}
	}
	client := d.live.client
	en := enabled
	msg := "已暂停全部路径"
	if enabled {
		msg = "已恢复全部路径"
	}
	d.mutate(func(ctx context.Context) error {
		var updatedList []engine.DeviceBinding
		var firstErr error
		for _, t := range targets {
			updated, err := client.SetBindingSync(ctx, t.id, api.BindingEdit{Revision: t.rev, Enabled: &en})
			if err != nil {
				firstErr = err
				break
			}
			if updated.ID != "" {
				updatedList = append(updatedList, updated)
			}
		}
		if len(updatedList) > 0 || firstErr != nil {
			done := make(map[string]bool, len(updatedList))
			for _, u := range updatedList {
				done[u.ID] = true
			}
			select {
			case d.live.results <- func() {
				for _, u := range updatedList {
					d.applyLiveBindingUpdate(u)
				}
				if firstErr != nil {
					for _, t := range targets {
						if done[t.id] {
							continue
						}
						if _, b := d.findDeviceBinding(t.id); b != nil && b.revision == t.rev {
							b.syncEnabled = t.prevEnabled
						}
					}
				}
			}:
				d.live.wake()
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return firstErr
	}, msg)
	if !d.live.busy {
		for _, t := range targets {
			if _, b := d.findDeviceBinding(t.id); b != nil && b.revision == t.rev {
				b.syncEnabled = t.prevEnabled
			}
		}
	}
}

func (d *viewState) updateLiveChannel(id string, u api.ChannelUpdate) {
	if d.live.busy {
		return
	}
	client := d.live.client
	hadPrev := false
	var prev externalChannel
	if ch := d.channelByID(id); ch != nil {
		hadPrev = true
		prev = *ch
		if u.Name != nil {
			ch.name = *u.Name
		}
		if u.Enabled != nil {
			ch.enabled = *u.Enabled
		}
		if u.AutoApply != nil {
			ch.autoApply = *u.AutoApply
		}
	}
	d.mutate(func(ctx context.Context) error {
		updated, err := client.UpdateChannel(ctx, id, u)
		if err == nil {
			if updated.ID != "" {
				select {
				case d.live.results <- func() { d.applyLiveChannelUpdate(updated) }:
					d.live.wake()
				case <-ctx.Done():
					return ctx.Err()
				}
			}
			return nil
		}
		if hadPrev {
			select {
			case d.live.results <- func() {
				if ch := d.channelByID(id); ch != nil && ch.revision == prev.revision {
					ch.name = prev.name
					ch.enabled = prev.enabled
					ch.autoApply = prev.autoApply
				}
			}:
				d.live.wake()
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return err
	}, "渠道设置已保存")
	if !d.live.busy && hadPrev {
		if ch := d.channelByID(id); ch != nil && ch.revision == prev.revision {
			ch.name = prev.name
			ch.enabled = prev.enabled
			ch.autoApply = prev.autoApply
		}
	}
}

func (d *viewState) enableLiveGlobalSync() {
	cfg := d.live.config.Config
	cfg.Sync.Enabled = true
	d.saveLiveConfig(cfg)
}

func (d *viewState) pairLive(input string, backend string) {
	client := d.live.client
	localBackend := d.live.pairingBackend
	d.fieldError = ""
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
		if err != nil {
			msg := liveErrorMessage(err)
			select {
			case d.live.results <- func() {
				if d.deviceTask == "connect" {
					d.fieldError = msg
				}
			}:
				d.live.wake()
			case <-ctx.Done():
				return ctx.Err()
			}
			return err
		}
		select {
		case d.live.results <- func() {
			d.live.join = &result
			d.fieldError = ""
			d.deviceTask = "wait"
		}:
			d.live.wake()
		case <-ctx.Done():
			return ctx.Err()
		}
		return nil
	}, "请求已发送，请核对指纹")
}

func (d *viewState) cancelLiveInvitation() {
	l := d.live
	l.ticket = ""
	l.code = api.Code{}
	d.inviteCode = ""
	d.clearPairingQR()
	l.enqueue(func(ctx context.Context) func() {
		_ = l.client.CancelCode(ctx)
		return func() { l.code = api.Code{}; l.ticket = ""; d.inviteCode = ""; d.clearPairingQR() }
	})
}

func (d *viewState) refreshLiveDevices(deviceID string) {
	if d.live == nil || d.live.busy || d.refreshingDevices {
		return
	}
	l := d.live
	client := l.client
	d.refreshingDevices = true
	d.refreshingDeviceID = deviceID
	l.message = ""
	go func() {
		rctx, cancel := context.WithTimeout(l.ctx, 12*time.Second)
		_, err := client.RefreshDevices(rctx, deviceID)
		cancel()
		apply := func() {
			d.refreshingDevices = false
			d.refreshingDeviceID = ""
			if err != nil {
				l.message = "无法刷新连接状态，请重试。"
				d.toastWarning(d.tr(l.message), desktopkit.ToastOptions{ID: toastIDLiveMessage})
			}
			select {
			case l.changed <- struct{}{}:
			default:
			}
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}
