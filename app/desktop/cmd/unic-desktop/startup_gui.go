package main

import (
	"context"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	paths "github.com/uniclipboard/unic/internal/localpaths"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func (g *gui) applyLoginSettings(requested, retry bool) {
	dir, err := paths.Dir()
	if err != nil {
		return
	}
	nativeAsync(func(ctx context.Context) error {
		if retry {
			if err := desktopprocess.Call(ctx, dir, desktopprocess.Tray, "retry-login", nil, nil); err != nil {
				return err
			}
		}
		var state roleStatus
		if err := desktopprocess.Call(ctx, dir, desktopprocess.Tray, "status", nil, &state); err != nil {
			return err
		}
		g.mu.Lock()
		g.loginEnabled = state.LoginEnabled
		g.loginError = state.LoginError
		g.mu.Unlock()
		if g.win != nil {
			g.win.Invalidate()
		}
		return nil
	})
}

func (g *gui) startupSettings(c *ui.Context, k desktopkit.Kit, state *config.State, busy bool) bool {
	changed := false
	desktop := &state.Config.Desktop
	g.mu.Lock()
	loginEnabled, loginError := g.loginEnabled, g.loginError
	g.mu.Unlock()
	k.SettingsGroup(c.Key("startup"), desktopkit.SettingsGroupProps{Title: "启动与运行模式"}, func() {
		k.SettingsRow(c.Key("mode"), desktopkit.SettingsRowProps{Label: "运行模式", Description: "常规模式关闭主窗口后保留托盘和同步服务。轻量模式关闭主窗口后仅保留同步服务。点击应用图标可重新打开。"}, nil, func() {
			if k.Select(c.Key("select"), desktopkit.SelectProps{Label: "运行模式", Value: &desktop.Mode, Disabled: busy, Items: []desktopkit.SelectItem{
				{Value: "normal", Label: "常规模式"}, {Value: "lightweight", Label: "轻量模式"},
			}}).Changed {
				changed = true
			}
		})
		k.SettingsRow(c.Key("login"), desktopkit.SettingsRowProps{Label: "登录时启动", Description: "登录系统后按所选运行模式启动。"}, nil, func() {
			if k.Switch(c.Key("switch"), &desktop.LaunchAtLogin, desktopkit.SwitchProps{Label: "登录时启动", Disabled: busy}).Changed() {
				changed = true
			}
		})
		k.SettingsRow(c.Key("background"), desktopkit.SettingsRowProps{Label: "后台启动", Description: "仅控制常规模式的登录启动。手动点击应用图标始终打开主窗口。"}, nil, func() {
			if k.Switch(c.Key("switch"), &desktop.StartInBackground, desktopkit.SwitchProps{Label: "后台启动", Disabled: busy}).Changed() {
				changed = true
			}
		})
		actual := "系统登录启动状态：已关闭"
		if loginEnabled {
			actual = "系统登录启动状态：已开启"
		}
		k.Text(c.Key("actual"), actual, desktopkit.TextProps{Role: desktopkit.RoleCaption})
		if loginError != "" {
			k.Text(c.Key("error"), loginError, desktopkit.TextProps{Tone: desktopkit.ToneCritical})
		}
		if desktop.LaunchAtLogin != loginEnabled || loginError != "" {
			if k.Button(c.Key("retry"), "重新应用登录启动设置", desktopkit.ButtonProps{Disabled: busy}).Clicked() {
				g.applyLoginSettings(desktop.LaunchAtLogin, true)
			}
		}
	})
	return changed
}
