package desktopui

import (
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

const (
	toastIDActionRun      = "action-run"
	toastIDRestart        = "restart-required"
	toastIDRestartError   = "restart-error"
	toastIDConfigInvalid  = "config-invalid"
	toastIDApplyError     = "config-apply-error"
	toastIDLiveMessage    = "live-message"
	toastIDPluginFeedback = "plugin-feedback"
	toastIDWeTypePair     = "wetype-pair"
	toastIDPendingDevices = "pending-devices"
)

// ShowToast pushes a notification into the shared BoardUI Toaster on the view thread.
func (s *Desktop) ShowToast(status desktopkit.ToastStatus, title string, opts ...desktopkit.ToastOptions) {
	if s == nil || s.d == nil || title == "" {
		return
	}
	var o desktopkit.ToastOptions
	if len(opts) > 0 {
		o = opts[0]
	}
	o.Status = status
	s.d.toasts.Show(title, o)
}

// ShowErrorToast pushes an error toast into the shared BoardUI Toaster on the view thread.
func (s *Desktop) ShowErrorToast(title string) {
	s.ShowToast(desktopkit.ToastError, title)
}

func (d *viewState) toastInfo(title string, opts ...desktopkit.ToastOptions) string {
	if title == "" {
		return ""
	}
	return d.toasts.Info(title, opts...)
}

func (d *viewState) toastSuccess(title string, opts ...desktopkit.ToastOptions) string {
	if title == "" {
		return ""
	}
	return d.toasts.Success(title, opts...)
}

func (d *viewState) toastWarning(title string, opts ...desktopkit.ToastOptions) string {
	if title == "" {
		return ""
	}
	return d.toasts.Warning(title, opts...)
}

func (d *viewState) toastError(title string, opts ...desktopkit.ToastOptions) string {
	if title == "" {
		return ""
	}
	return d.toasts.Error(title, opts...)
}

func (d *viewState) toastDismiss(id string) {
	d.toasts.Dismiss(id)
}

// syncStateToasts reconciles persistent service/configuration states (config
// errors, apply failures, and restart prompts) with the unified BoardUI Toaster.
// Manual dismissal is respected until the underlying condition changes.
func (d *viewState) syncStateToasts() {
	l := d.live
	if l == nil {
		d.toasts.Dismiss(toastIDApplyError)
		d.toasts.Dismiss(toastIDConfigInvalid)
		d.toasts.Dismiss(toastIDPendingDevices)
		d.syncRestartToast(false, false, "", "")
		return
	}

	// 1. Config apply error (saved but failed to apply).
	if l.config.ApplyError != "" {
		if d.dismissedApply != l.config.ApplyError {
			key := l.config.ApplyError
			opts := desktopkit.ToastOptions{
				Status: desktopkit.ToastError,
				OnDismiss: func() {
					d.dismissedApply = key
				},
			}
			if l.restartApp != nil {
				opts.Actions = []desktopkit.ToastAction{{
					Label:    d.tr("立即重启"),
					Variant:  "primary",
					Busy:     l.restarting,
					Disabled: l.restarting,
					KeepOpen: true,
					OnClick:  func() { d.requestRestart(true) },
				}}
			}
			d.toasts.SyncPersistent(toastIDApplyError, d.tr("设置已保存，但未能生效。请重试或重新启动桌面端。"), opts)
		}
	} else {
		d.dismissedApply = ""
		d.toasts.Dismiss(toastIDApplyError)
	}

	// 2. Invalid config file warning.
	if l.config.Error != "" {
		if d.dismissedCfgErr != l.config.Error {
			key := l.config.Error
			d.toasts.SyncPersistent(toastIDConfigInvalid, d.tr("配置文件无效。当前保留上一份有效设置。"), desktopkit.ToastOptions{
				Status: desktopkit.ToastWarning,
				OnDismiss: func() {
					d.dismissedCfgErr = key
				},
			})
		}
	} else {
		d.dismissedCfgErr = ""
		d.toasts.Dismiss(toastIDConfigInvalid)
	}

	// 3. Restart required warning (devices sync channels, plugins, or general settings).
	pluginsPending := false
	for _, key := range l.config.RestartRequired {
		if key == "plugins" || strings.HasPrefix(key, "plugins.") {
			pluginsPending = true
			break
		}
	}
	p := &d.pluginSettings
	if external, ok := d.installedExternalPlugin(p.id); ok && (external.Restart || (p.form != nil && p.form.state.Restart)) {
		pluginsPending = true
	}
	restartService := len(l.config.RestartRequired) > 0 || pluginsPending
	windowFrameMismatch := d.windowFrame != nil && *d.windowFrame != l.config.Config.Desktop.SystemWindowFrame

	switch {
	case d.page == "devices" && d.restartPending:
		if len(d.runningSyncChannels()) == 0 {
			d.syncRestartToast(true, true, "devices-zero", d.tr("同步渠道配置已更改，请退出并重新启动桌面端以生效。"))
		} else {
			d.syncRestartToast(true, true, "devices-running", d.tr("同步渠道设置已保存，请退出并重新启动桌面端以生效。"))
		}
	case pluginsPending && (d.page == "settings" && d.settings.Value == "plugins" || !windowFrameMismatch):
		d.syncRestartToast(true, true, "plugins", d.tr("插件设置已保存。请退出并重新启动桌面端以生效。"))
	case restartService || windowFrameMismatch:
		scope := "window"
		if restartService {
			scope = "service"
		}
		d.syncRestartToast(true, restartService, scope, d.tr("部分设置将在下次启动时生效。"))
	default:
		d.syncRestartToast(false, false, "", "")
	}

	// 4. Pending device approval guidance when viewing other pages.
	d.syncPendingDeviceToast()
}

func (d *viewState) syncPendingDeviceToast() {
	l := d.live
	if l == nil || len(l.pending) == 0 || len(d.pending) == 0 {
		d.dismissedPending = ""
		d.toasts.Dismiss(toastIDPendingDevices)
		return
	}
	if d.page == "devices" {
		d.toasts.Dismiss(toastIDPendingDevices)
		return
	}
	keys := make([]string, 0, len(d.pending))
	for _, p := range d.pending {
		keys = append(keys, fmt.Sprintf("%s:%d", p.id, p.revision))
	}
	slices.Sort(keys)
	key := strings.Join(keys, ",")
	if d.dismissedPending == key {
		return
	}
	first := d.pending[0]
	var title string
	if len(d.pending) == 1 {
		if d.isExistingPendingDevice(first.id) {
			title = fmt.Sprintf("%s %s", first.name, d.tr("请求新增同步路径"))
		} else {
			title = fmt.Sprintf("%s %s", first.name, d.tr("请求加入同步网络"))
		}
	} else {
		title = fmt.Sprintf(d.tr("%d 台设备正在等待批准"), len(d.pending))
	}
	targetID := first.id
	d.toasts.SyncPersistent(toastIDPendingDevices, title, desktopkit.ToastOptions{
		Status:      desktopkit.ToastInformation,
		Description: d.tr("请前往设备管理核对双端指纹后批准。"),
		OnDismiss: func() {
			d.dismissedPending = key
		},
		Actions: []desktopkit.ToastAction{{
			Label:   d.tr("前往核对"),
			Variant: "primary",
			OnClick: func() {
				d.openPendingDevice(targetID)
			},
		}},
	})
}

func (d *viewState) syncRestartToast(active, restartService bool, key, title string) {
	if !active {
		d.dismissedRestart = ""
		d.toasts.Dismiss(toastIDRestart)
		return
	}
	if d.dismissedRestart == key {
		return
	}
	opts := desktopkit.ToastOptions{
		Status: desktopkit.ToastWarning,
		OnDismiss: func() {
			d.dismissedRestart = key
		},
	}
	if d.live != nil && d.live.restartApp != nil {
		restarting := d.live.restarting
		opts.Actions = []desktopkit.ToastAction{{
			Label:    d.tr("立即重启"),
			Variant:  "primary",
			Busy:     restarting,
			Disabled: restarting,
			KeepOpen: true,
			OnClick:  func() { d.requestRestart(restartService) },
		}}
	}
	d.toasts.SyncPersistent(toastIDRestart, title, opts)
}

func (d *viewState) toasterOverlay(c *ui.Context, k desktopkit.Kit) {
	d.syncStateToasts()
	k.Toaster(c, &d.toasts, desktopkit.ToasterProps{
		Position:   desktopkit.ToastBottomCenter,
		Limit:      desktopkit.DefaultToastLimit,
		Duration:   desktopkit.DefaultToastDuration,
		Label:      d.tr("通知"),
		CloseLabel: d.tr("关闭提示"),
	})
}
