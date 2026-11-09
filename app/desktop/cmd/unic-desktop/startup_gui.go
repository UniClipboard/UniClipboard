package main

import (
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func (g *gui) applyLoginSettings(requested, retry bool) {
	// Run the whole reconciliation on the native thread to avoid lock/dispatch inversion.
	mygo.RunOnMain(func() {
		enabled, err := g.login.Apply(requested, retry)
		g.mu.Lock()
		g.loginEnabled = enabled
		g.loginError = ""
		if err != nil {
			g.loginError = "登录启动设置未生效。请检查系统登录项权限。macOS 需要应用包，自定义配置目录不支持登录启动。"
		}
		g.mu.Unlock()
		if g.win != nil {
			g.win.Invalidate()
		}
	})
}

func (g *gui) startupSettings(c *ui.Context, k uikit.Kit, state *config.State, busy bool) bool {
	changed := false
	desktop := &state.Config.Desktop
	g.mu.Lock()
	loginEnabled, loginError := g.loginEnabled, g.loginError
	g.mu.Unlock()
	k.SettingsGroup(c.Key("startup"), uikit.SettingsGroupProps{Title: "启动与运行模式"}, func() {
		k.SettingsRow(c.Key("mode"), uikit.SettingsRowProps{Label: "运行模式", Description: "下次启动时生效。轻量模式只运行同步服务。"}, nil, func() {
			if k.Select(c.Key("select"), uikit.SelectProps{Label: "运行模式", Value: &desktop.Mode, Disabled: busy, Items: []uikit.SelectItem{
				{Value: "normal", Label: "常规模式"}, {Value: "lightweight", Label: "轻量模式"},
			}}).Changed {
				changed = true
			}
		})
		k.SettingsRow(c.Key("login"), uikit.SettingsRowProps{Label: "登录时启动", Description: "登录系统后按所选运行模式启动。"}, nil, func() {
			if k.Switch(c.Key("switch"), &desktop.LaunchAtLogin, uikit.SwitchProps{Label: "登录时启动", LabelHidden: true, Disabled: busy}).Changed() {
				changed = true
			}
		})
		k.SettingsRow(c.Key("background"), uikit.SettingsRowProps{Label: "后台启动", Description: "下次常规启动时隐藏主窗口。通过托盘或再次启动打开窗口。"}, nil, func() {
			if k.Switch(c.Key("switch"), &desktop.StartInBackground, uikit.SwitchProps{Label: "后台启动", LabelHidden: true, Disabled: busy}).Changed() {
				changed = true
			}
		})
		actual := "系统登录启动状态：已关闭"
		if loginEnabled {
			actual = "系统登录启动状态：已开启"
		}
		k.Text(c.Key("actual"), actual, uikit.TextProps{Role: uikit.RoleCaption})
		if loginError != "" {
			k.Text(c.Key("error"), loginError, uikit.TextProps{Tone: uikit.ToneCritical})
		}
		if desktop.LaunchAtLogin != loginEnabled || loginError != "" {
			if k.Button(c.Key("retry"), "重新应用登录启动设置", uikit.ButtonProps{Disabled: busy}).Clicked() {
				g.applyLoginSettings(desktop.LaunchAtLogin, true)
			}
		}
	})
	return changed
}
