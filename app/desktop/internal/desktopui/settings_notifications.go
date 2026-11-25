package desktopui

import (
	"context"
	"errors"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// notificationSettingsState manages passive OS notification permission reads,
// explicit System Settings navigation, and user-initiated test notifications.
type notificationSettingsState struct {
	read       func(context.Context) (string, error)
	open       func(context.Context) error
	sendTest   func(context.Context) error
	status     string
	loaded     bool
	loading    bool
	opening    bool
	testing    bool
	generation uint64
	nextRead   time.Time
}

// SetNotificationActions binds host-owned passive notification status reading,
// opening OS notification settings, and sending an explicit test notification.
// Call on the view thread. Callbacks run on independent workers.
func (s *Desktop) SetNotificationActions(
	read func(context.Context) (string, error),
	open func(context.Context) error,
	sendTest func(context.Context) error,
) {
	previous := s.d.notifications.generation
	s.d.notifications = notificationSettingsState{
		read:       read,
		open:       open,
		sendTest:   sendTest,
		status:     "unknown",
		generation: previous + 1,
	}
}

func (d *viewState) refreshNotificationPermission() {
	l, s := d.live, &d.notifications
	if l == nil || l.ctx.Err() != nil || s.loading {
		return
	}
	if s.read == nil {
		s.status, s.loaded = "unsupported", true
		return
	}
	s.loading = true
	read, generation := s.read, s.generation
	go func() {
		ctx, cancel := context.WithTimeout(l.ctx, 5*time.Second)
		defer cancel()
		status, err := read(ctx)
		if ctx.Err() != nil {
			err = ctx.Err()
		}
		apply := func() {
			if l.ctx.Err() != nil || s.generation != generation {
				return
			}
			s.loading, s.loaded = false, true
			s.nextRead = time.Now().Add(5 * time.Second)
			if err != nil {
				s.status = "unknown"
				d.toastWarning(d.tr("通知权限状态读取失败，请重试。"), desktopkit.ToastOptions{ID: "notif-perm"})
				return
			}
			switch status {
			case "granted", "denied", "not_determined", "supported", "unsupported", "unknown":
				s.status = status
			default:
				s.status = "unknown"
			}
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

func (d *viewState) openNotificationSystemSettings() {
	l, s := d.live, &d.notifications
	if l == nil || l.ctx.Err() != nil || s.opening || s.open == nil {
		return
	}
	s.opening = true
	open, generation := s.open, s.generation
	go func() {
		ctx, cancel := context.WithTimeout(l.ctx, 5*time.Second)
		defer cancel()
		err := open(ctx)
		if ctx.Err() != nil {
			err = ctx.Err()
		}
		apply := func() {
			if l.ctx.Err() != nil || s.generation != generation {
				return
			}
			s.opening = false
			if err != nil {
				d.toastError(d.tr("无法打开系统通知设置，请手动在系统设置中查看。"), desktopkit.ToastOptions{ID: "notif-perm-open"})
				return
			}
			d.toastInfo(d.tr("已请求打开系统通知设置。调整后请点击刷新查看最新状态。"), desktopkit.ToastOptions{ID: "notif-perm-open"})
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

func (d *viewState) sendTestNotification() {
	l, s := d.live, &d.notifications
	if l == nil || l.ctx.Err() != nil || s.testing || s.sendTest == nil {
		return
	}
	s.testing = true
	sendTest, generation := s.sendTest, s.generation
	go func() {
		ctx, cancel := context.WithTimeout(l.ctx, 8*time.Second)
		defer cancel()
		err := sendTest(ctx)
		if ctx.Err() != nil {
			err = ctx.Err()
		}
		apply := func() {
			if l.ctx.Err() != nil || s.generation != generation {
				return
			}
			s.testing = false
			if err != nil {
				if errors.Is(err, errors.ErrUnsupported) {
					d.toastWarning(d.tr("当前运行环境不支持系统通知。"), desktopkit.ToastOptions{ID: "notif-test"})
				} else {
					d.toastWarning(d.tr("测试通知发送失败，请检查系统通知权限后重试。"), desktopkit.ToastOptions{ID: "notif-test"})
				}
			} else {
				d.toastSuccess(d.tr("已发送测试通知。"), desktopkit.ToastOptions{ID: "notif-test"})
			}
			s.nextRead = time.Time{}
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

func (d *viewState) notificationSettingsContent(c *ui.Context, k desktopkit.Kit, cfg *config.Config, changed *bool) {
	l := d.live
	s := &d.notifications
	if !s.loading && (!s.loaded || time.Now().After(s.nextRead)) {
		d.refreshNotificationPermission()
	}

	masterOn := cfg.Desktop.Notifications.EnabledOrDefault()
	pairingOn := cfg.Desktop.Notifications.DevicePairing == nil || *cfg.Desktop.Notifications.DevicePairing

	k.SettingsGroup(c.Key("notification-switches"), desktopkit.SettingsGroupProps{Title: d.tr("通知开关")}, func() {
		k.SettingsRow(c.Key("notif-enabled"), desktopkit.SettingsRowProps{
			Label:       d.tr("系统通知"),
			Description: d.tr("接收来自 UniClipboard 的系统级桌面通知。关闭后仍会在应用内保留红点提醒。"),
		}, nil, func() {
			val := masterOn
			if k.Switch(c, &val, desktopkit.SwitchProps{Label: d.tr("系统通知"), Disabled: l.busy}).Changed() {
				v := val
				cfg.Desktop.Notifications.Enabled = &v
				*changed = true
			}
		})
		k.SettingsRow(c.Key("notif-device-pairing"), desktopkit.SettingsRowProps{
			Label:       d.tr("新设备配对与路径请求"),
			Description: d.tr("有新设备请求加入或新增同步路径时发送系统通知。"),
			Dim:         !masterOn,
		}, nil, func() {
			val := pairingOn
			if k.Switch(c, &val, desktopkit.SwitchProps{Label: d.tr("新设备配对与路径请求"), Disabled: l.busy || !masterOn}).Changed() {
				v := val
				cfg.Desktop.Notifications.DevicePairing = &v
				*changed = true
			}
		})
	})

	label := d.tr("状态未知")
	tone := desktopkit.StatusNeutral
	switch s.status {
	case "granted":
		label = d.tr("已授权")
		tone = desktopkit.StatusPositive
	case "supported":
		label = d.tr("可用")
		tone = desktopkit.StatusPositive
	case "not_determined":
		label = d.tr("尚未授权")
		tone = desktopkit.StatusNeutral
	case "denied":
		label = d.tr("未授权")
		tone = desktopkit.StatusWarning
	case "unsupported":
		label = d.tr("当前环境不支持")
		tone = desktopkit.StatusNeutral
	}
	if s.loading {
		label = d.tr("正在读取权限状态…")
		tone = desktopkit.StatusNeutral
	}

	desc := d.tr("仅读取当前应用的系统通知状态，不会主动弹窗。点击「发送测试通知」可验证通知送达或触发首次系统授权。")
	hint := d.tr("如在系统设置中更改了通知权限，请点击刷新以更新状态。")
	switch s.status {
	case "unsupported":
		desc = d.tr("仅打包后的桌面应用支持系统通知；应用内红点与弹窗指引仍会正常显示。")
		hint = d.tr("当前运行环境不支持原生系统通知。")
	case "denied":
		hint = d.tr("系统已关闭本应用通知权限，请在系统设置中重新允许。")
	}

	k.SettingsGroup(c.Key("notification-permissions"), desktopkit.SettingsGroupProps{Title: d.tr("系统通知权限")}, func() {
		k.SettingsRow(c.Key("notif-perm-status"), desktopkit.SettingsRowProps{
			Label:       d.tr("系统通知状态"),
			Description: desc,
		}, nil, func() {
			settingsPill(c.Key("status-pill"), k, label, tone, true, false)
		})
		settingsFooterBar(c.Key("notif-perm-actions"), k, hint, s.status == "denied", func() {
			if k.Button(c.Key("refresh"), d.tr("刷新权限状态"), desktopkit.ButtonProps{
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconRetry,
				Disabled: s.loading,
				Busy:     s.loading,
			}).Clicked() {
				d.refreshNotificationPermission()
			}
			if s.status != "unsupported" {
				if k.Button(c.Key("test"), d.tr("发送测试通知"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Disabled: s.testing || s.sendTest == nil,
					Busy:     s.testing,
				}).Clicked() {
					d.sendTestNotification()
				}
			}
			if s.open != nil && s.status != "unsupported" {
				variant := desktopkit.Standard
				if s.status == "denied" {
					variant = desktopkit.Primary
				}
				if k.Button(c.Key("open-settings"), d.tr("打开系统设置"), desktopkit.ButtonProps{
					Variant:  variant,
					Size:     desktopkit.SizeSmall,
					Disabled: s.opening,
				}).Clicked() {
					d.openNotificationSystemSettings()
				}
			}
		})
	})
}
