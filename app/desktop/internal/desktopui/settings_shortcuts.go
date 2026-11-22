package desktopui

import (
	"context"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

// shortcutSettingsState contains a passive permission receipt, not a promise
// that native target capture or paste is available. The native host owns the
// actual permission check and the explicit operating-system settings action.
type shortcutSettingsState struct {
	read       func(context.Context) (string, error)
	open       func(context.Context) error
	status     string
	err        string
	actionErr  string
	notice     string
	loaded     bool
	loading    bool
	opening    bool
	generation uint64
	nextRead   time.Time
}

// SetShortcutPermissionActions binds the installed executable's permission
// status and explicit settings action. Call on the view thread. Callbacks run
// on independent workers and must not mutate view state. A read returns one of
// granted, denied, unknown or unsupported. Neither callback requests a grant.
func (s *Desktop) SetShortcutPermissionActions(read func(context.Context) (string, error), open func(context.Context) error) {
	previous := s.d.shortcuts.generation
	s.d.shortcuts = shortcutSettingsState{read: read, open: open, status: "unknown", generation: previous + 1}
}

func (d *viewState) refreshShortcutPermission() {
	l, s := d.live, &d.shortcuts
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
			s.err = ""
			if err != nil {
				s.status = "unknown"
				s.err = d.tr("权限状态读取失败，请重试。")
				return
			}
			switch status {
			case "granted", "denied", "unknown", "unsupported":
				s.status = status
			default:
				s.status = "unknown"
				s.err = d.tr("权限状态读取失败，请重试。")
			}
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

func (d *viewState) openShortcutPermissionSettings() {
	l, s := d.live, &d.shortcuts
	if l == nil || l.ctx.Err() != nil || s.opening || s.open == nil {
		return
	}
	s.opening, s.notice, s.actionErr = true, "", ""
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
				s.actionErr = d.tr("无法打开系统设置，请重试。")
				return
			}
			// Opening a settings page is never evidence that permission changed.
			s.notice = d.tr("已请求打开系统设置。请完成授权后刷新状态。")
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

func (d *viewState) shortcutPermissionSettings(c *ui.Context, k desktopkit.Kit) {
	s := &d.shortcuts
	if !s.loading && (!s.loaded || time.Now().After(s.nextRead)) {
		d.refreshShortcutPermission()
	}
	label := d.tr("状态未知")
	tone := desktopkit.StatusNeutral
	switch s.status {
	case "granted":
		label = d.tr("已授权")
		tone = desktopkit.StatusPositive
	case "denied":
		label = d.tr("未授权")
		tone = desktopkit.StatusWarning
	case "unsupported":
		label = d.tr("此平台没有此权限开关")
		tone = desktopkit.StatusNeutral
	}
	if s.loading {
		label = d.tr("正在读取权限状态…")
		tone = desktopkit.StatusNeutral
	}
	k.SettingsGroup(c.Key("shortcut-permissions"), desktopkit.SettingsGroupProps{Title: d.tr("系统权限")}, func() {
		k.SettingsRow(c.Key("accessibility-status"), desktopkit.SettingsRowProps{
			Label:       d.tr("辅助功能权限（macOS）"),
			Description: d.tr("仅显示当前应用的系统授权状态，读取不会弹出授权框；此状态不代表目标应用一定支持自动粘贴。"),
		}, nil, func() {
			settingsPill(c.Key("status-pill"), k, label, tone, true, false)
		})
		if s.err != "" {
			settingsStrip(c.Key("read-error"), k, desktopkit.ToneWarning, s.err)
		}
		if s.actionErr != "" {
			settingsStrip(c.Key("action-error"), k, desktopkit.ToneWarning, s.actionErr)
		}
		if s.notice != "" {
			settingsStrip(c.Key("notice"), k, desktopkit.ToneInfo, s.notice)
		}
		hint := d.tr("授权完成后请手动点击刷新，以读取系统最新状态。")
		if s.status == "unsupported" {
			hint = d.tr("当前平台无需 macOS 辅助功能权限开关。")
		}
		settingsFooterBar(c.Key("permission-actions"), k, hint, false, func() {
			if k.Button(c.Key("refresh"), d.tr("刷新权限状态"), desktopkit.ButtonProps{
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconRetry,
				Disabled: s.loading,
				Busy:     s.loading,
			}).Clicked() {
				d.refreshShortcutPermission()
			}
			if s.status != "unsupported" {
				variant := desktopkit.Standard
				if s.status == "denied" {
					variant = desktopkit.Primary
				}
				if k.Button(c.Key("open-settings"), d.tr("打开系统设置"), desktopkit.ButtonProps{
					Variant:  variant,
					Size:     desktopkit.SizeSmall,
					Disabled: s.opening || s.open == nil,
				}).Clicked() {
					d.openShortcutPermissionSettings()
				}
			}
		})
	})
}
