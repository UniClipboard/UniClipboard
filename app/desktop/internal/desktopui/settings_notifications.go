package desktopui

import (
	"context"
	"errors"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

const notificationPermissionRefreshInterval = 5 * time.Second

// notificationSettingsState manages passive OS notification permission reads,
// explicit System Settings navigation, and user-initiated test notifications.
type notificationSettingsState struct {
	read             func(context.Context) (string, error)
	open             func(context.Context) error
	sendTest         func(context.Context) error
	status           string
	loaded           bool
	loading          bool
	opening          bool
	testing          bool
	generation       uint64
	scheduleNextRead bool
	nextRead         time.Time
	firstReadAt      time.Time
	manualReadAt     time.Time
	openingAt        time.Time
	testingAt        time.Time
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

// A zero manualAt marks a passive read with no busy presentation.
func (d *viewState) refreshNotificationPermission(now, manualAt time.Time) {
	l, s := d.live, &d.notifications
	if l == nil || l.ctx.Err() != nil {
		return
	}
	if s.loading {
		if !manualAt.IsZero() && s.manualReadAt.IsZero() {
			s.manualReadAt = manualAt
		}
		return
	}
	if s.read == nil {
		s.status, s.loaded = "unsupported", true
		return
	}
	s.loading = true
	s.scheduleNextRead = false
	if !s.loaded && s.firstReadAt.IsZero() {
		s.firstReadAt = now
	}
	s.manualReadAt = manualAt
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
			s.manualReadAt = time.Time{}
			s.scheduleNextRead = true
			if err != nil {
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

func (d *viewState) openNotificationSystemSettings(now time.Time) {
	l, s := d.live, &d.notifications
	if l == nil || l.ctx.Err() != nil || s.opening || s.open == nil {
		return
	}
	s.opening = true
	s.openingAt = now
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
			s.openingAt = time.Time{}
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

func (d *viewState) sendTestNotification(now time.Time) {
	l, s := d.live, &d.notifications
	if l == nil || l.ctx.Err() != nil || s.testing || s.sendTest == nil {
		return
	}
	s.testing = true
	s.testingAt = now
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
			s.testingAt = time.Time{}
			if err != nil {
				if errors.Is(err, errors.ErrUnsupported) {
					d.toastWarning(d.tr("当前运行环境不支持系统通知。"), desktopkit.ToastOptions{ID: "notif-test"})
				} else {
					d.toastWarning(d.tr("测试通知发送失败，请检查系统通知权限后重试。"), desktopkit.ToastOptions{ID: "notif-test"})
				}
			} else {
				d.toastSuccess(d.tr("已发送测试通知。"), desktopkit.ToastOptions{ID: "notif-test"})
			}
			s.scheduleNextRead = false
			s.nextRead = time.Time{}
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

func delayedActionBusy(c *ui.Context, now, startedAt time.Time, active bool) bool {
	if !active || startedAt.IsZero() {
		return false
	}
	elapsed := now.Sub(startedAt)
	if elapsed >= foregroundLoadingIndicatorDelay {
		return true
	}
	c.After(foregroundLoadingIndicatorDelay - elapsed)
	return false
}

func (d *viewState) notificationSettingsContent(c *ui.Context, k desktopkit.Kit, cfg *config.Config, changed *bool) {
	l := d.live
	s := &d.notifications
	now := c.Now()
	if s.scheduleNextRead {
		s.scheduleNextRead = false
		s.nextRead = now.Add(notificationPermissionRefreshInterval)
	}
	if !s.loading && (!s.loaded || (s.read != nil && !now.Before(s.nextRead))) {
		d.refreshNotificationPermission(now, time.Time{})
	}

	masterOn := cfg.Desktop.Notifications.EnabledOrDefault()
	pairingOn := cfg.Desktop.Notifications.DevicePairing == nil || *cfg.Desktop.Notifications.DevicePairing

	k.SettingsGroup(c.Key("notification-switches"), desktopkit.SettingsGroupProps{Title: d.tr("通知开关")}, func() {
		k.SettingsRow(c.Key("notif-enabled"), desktopkit.SettingsRowProps{
			Label:       d.tr("系统通知"),
			Description: d.tr("接收来自 Unic 的系统级桌面通知。关闭后仍会在应用内保留红点提醒。"),
		}, nil, func() {
			val := masterOn
			if k.Switch(c, &val, desktopkit.SwitchProps{Label: d.tr("系统通知"), Disabled: l.showBusy()}).Changed() && !l.busy {
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
			if k.Switch(c, &val, desktopkit.SwitchProps{Label: d.tr("新设备配对与路径请求"), Disabled: l.showBusy() || !masterOn}).Changed() && !l.busy {
				v := val
				cfg.Desktop.Notifications.DevicePairing = &v
				*changed = true
			}
		})
	})

	label := d.tr("状态未知")
	tone := desktopkit.StatusNeutral
	footer := d.tr("如在系统设置中更改了通知权限，请点击刷新以更新状态。")
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
		footer = d.tr("系统已关闭本应用通知权限，请在系统设置中重新允许。")
	case "unsupported":
		label = d.tr("当前环境不支持")
		footer = d.tr("仅打包后的桌面应用支持系统通知；应用内红点与弹窗指引仍会正常显示。")
	}

	unsupported := s.status == "unsupported"
	showPill := s.loaded || delayedActionBusy(c, now, s.firstReadAt, s.loading)
	refreshBusy := delayedActionBusy(c, now, s.manualReadAt, s.loading)
	openBusy := delayedActionBusy(c, now, s.openingAt, s.opening)
	testBusy := delayedActionBusy(c, now, s.testingAt, s.testing)

	openVariant := desktopkit.Standard
	if s.status == "denied" {
		openVariant = desktopkit.Primary
	}

	k.SettingsGroup(c.Key("notification-permissions"), desktopkit.SettingsGroupProps{
		Title:  d.tr("系统通知权限"),
		Footer: footer,
		Trailing: func() {
			if showPill {
				settingsPill(c.Key("status-pill"), k, label, tone, true, false)
			}
			if k.Button(c.Key("refresh"), d.tr("刷新"), desktopkit.ButtonProps{
				Variant:  desktopkit.Subtle,
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconRetry,
				Label:    d.tr("刷新权限状态"),
				Tooltip:  d.tr("刷新权限状态"),
				Disabled: s.read == nil,
				Busy:     refreshBusy,
			}).Clicked() {
				d.refreshNotificationPermission(now, now)
			}
		},
	}, func() {
		k.SettingsRow(c.Key("notif-open-settings"), desktopkit.SettingsRowProps{
			Label:       d.tr("系统通知设置"),
			Description: d.tr("前往系统设置管理本应用的通知权限与提醒方式。"),
			Dim:         unsupported,
		}, nil, func() {
			if k.Button(c.Key("open-settings"), d.tr("打开系统设置"), desktopkit.ButtonProps{
				Variant:  openVariant,
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconSettings,
				Disabled: unsupported || s.open == nil,
				Busy:     openBusy,
			}).Clicked() {
				d.openNotificationSystemSettings(now)
			}
		})
		k.SettingsRow(c.Key("notif-send-test"), desktopkit.SettingsRowProps{
			Label:       d.tr("测试通知"),
			Description: d.tr("发送一条测试通知，验证通知送达或触发首次系统授权。"),
			Dim:         unsupported,
		}, nil, func() {
			if k.Button(c.Key("test"), d.tr("发送测试通知"), desktopkit.ButtonProps{
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconSend,
				Disabled: unsupported || s.sendTest == nil,
				Busy:     testBusy,
			}).Clicked() {
				d.sendTestNotification(now)
			}
		})
	})
}
