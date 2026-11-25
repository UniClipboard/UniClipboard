package desktopui

import (
	"context"
	"time"

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

func (d *viewState) renameLivePeer(id string, rev int64, name string) {
	client := d.live.client
	msg := "设备名称已保存"
	if name == "" {
		msg = "已恢复为对方自报名"
	}
	d.mutate(func(ctx context.Context) error {
		_, err := client.RenamePeer(ctx, id, api.PeerRename{Revision: rev, Name: name})
		return err
	}, msg)
}

func (d *viewState) setLiveBindingSync(bindingID string, rev int64, enabled bool) {
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		_, err := client.SetBindingSync(ctx, bindingID, api.BindingEdit{Revision: rev, Enabled: enabled})
		return err
	}, "同步路径设置已保存")
}

func (d *viewState) setLiveAllBindingsSync(bindings []deviceBinding, enabled bool) {
	type target struct {
		id  string
		rev int64
	}
	var targets []target
	for _, b := range bindings {
		if b.state == "authorized" && b.syncEnabled != enabled {
			targets = append(targets, target{id: b.id, rev: b.revision})
		}
	}
	if len(targets) == 0 {
		return
	}
	client := d.live.client
	msg := "已暂停全部路径"
	if enabled {
		msg = "已恢复全部路径"
	}
	d.mutate(func(ctx context.Context) error {
		for _, t := range targets {
			if _, err := client.SetBindingSync(ctx, t.id, api.BindingEdit{Revision: t.rev, Enabled: enabled}); err != nil {
				return err
			}
		}
		return nil
	}, msg)
}

func (d *viewState) updateLiveChannel(id string, u api.ChannelUpdate) {
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		_, err := client.UpdateChannel(ctx, id, u)
		return err
	}, "渠道设置已保存")
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
	l.enqueue(func(ctx context.Context) func() {
		_ = l.client.CancelCode(ctx)
		return func() { l.code = api.Code{}; l.ticket = ""; d.inviteCode = "" }
	})
}

func (d *viewState) refreshLiveDevices(deviceID string) {
	if d.live == nil || d.refreshingDevices {
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
