package main

import (
	"bytes"
	"context"
	"slices"
	"sort"
	"strconv"
	"time"

	"github.com/BurntSushi/toml"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type cleanupUIState struct {
	state        config.State
	tags         []string
	loaded, busy bool
	message      string
}

func (g *gui) loadCleanupSettings() {
	g.mu.Lock()
	if g.cleanupUI.busy {
		g.mu.Unlock()
		return
	}
	g.cleanupUI.busy = true
	g.cleanupUI.message = "正在读取设置。"
	g.mu.Unlock()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		state, err := g.c.Config(ctx)
		var tags []string
		if err == nil {
			tags, err = g.c.HistoryTags(ctx)
		}
		g.mu.Lock()
		g.cleanupUI.busy = false
		if err != nil {
			g.cleanupUI.message = "无法读取设置，请重试。"
		} else {
			g.cleanupUI.state = state
			g.cleanupUI.tags = tags
			g.cleanupUI.loaded = true
			g.cleanupUI.message = ""
		}
		g.mu.Unlock()
		g.win.Invalidate()
	}()
}

func (g *gui) saveCleanupSettings(state config.State) {
	g.mu.Lock()
	if g.cleanupUI.busy {
		g.mu.Unlock()
		return
	}
	g.cleanupUI.busy = true
	g.cleanupUI.message = "正在保存。"
	g.mu.Unlock()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		var raw bytes.Buffer
		err := toml.NewEncoder(&raw).Encode(state.Config)
		if err == nil {
			state, err = g.c.SaveConfig(ctx, state.Revision, raw.Bytes())
		}
		g.mu.Lock()
		g.cleanupUI.busy = false
		switch {
		case err != nil:
			g.cleanupUI.message = "保存失败，请重新读取设置后重试。"
		case state.ApplyError != "":
			g.cleanupUI.state = state
			g.cleanupUI.message = "设置已保存，但未能生效。"
		default:
			g.cleanupUI.state = state
			g.cleanupUI.message = "已保存并生效。"
		}
		g.mu.Unlock()
		g.win.Invalidate()
	}()
}

func cleanupChoices(value int, values []int, suffix string) []uikit.SelectItem {
	if !slices.Contains(values, value) {
		values = append(slices.Clone(values), value)
	}
	out := make([]uikit.SelectItem, 0, len(values))
	for _, n := range values {
		label := strconv.Itoa(n) + suffix
		if n == 0 {
			label = "无限制"
		}
		out = append(out, uikit.SelectItem{Value: strconv.Itoa(n), Label: label})
	}
	return out
}

func (g *gui) cleanupSettingsTab(c *ui.Context, k uikit.Kit) {
	g.mu.Lock()
	view := g.cleanupUI
	state := view.state
	state.Config.History.ExcludedTags = slices.Clone(state.Config.History.ExcludedTags)
	g.mu.Unlock()
	if !view.loaded {
		if !view.busy && view.message == "" {
			g.loadCleanupSettings()
		}
		k.Text(c, "正在读取设置", uikit.TextProps{})
	}
	k.SettingsContent(c, uikit.SettingsContentProps{Label: "自动清理设置"}, func() {
		if view.message != "" {
			k.Text(c.Key("message"), view.message, uikit.TextProps{})
		}
		if k.Button(c.Key("reload"), "重新读取", uikit.ButtonProps{Disabled: view.busy}).Clicked() {
			g.loadCleanupSettings()
		}
		if !view.loaded {
			return
		}
		h := &state.Config.History
		disabled := view.busy || !h.AutoCleanup
		changed := false
		k.SettingsGroup(c.Key("history"), uikit.SettingsGroupProps{Title: "历史保留"}, func() {
			k.SettingsRow(c.Key("enabled"), uikit.SettingsRowProps{Label: "自动清理历史", Description: "按保留规则清理历史记录及应用管理的文件"}, nil, func() {
				if k.Switch(c.Key("switch"), &h.AutoCleanup, uikit.SwitchProps{Label: "自动清理历史", LabelHidden: true, Disabled: view.busy}).Changed() {
					changed = true
				}
			})
			k.SettingsRow(c.Key("age"), uikit.SettingsRowProps{Label: "历史保留时间", Description: "从历史记录创建时间计算"}, nil, func() {
				value := strconv.Itoa(h.RetentionDays)
				if k.Select(c.Key("select"), uikit.SelectProps{Label: "历史保留时间", Value: &value, Items: cleanupChoices(h.RetentionDays, []int{7, 30, 90, 180, 365, 0}, " 天"), Disabled: disabled}).Changed {
					h.RetentionDays, _ = strconv.Atoi(value)
					changed = true
				}
			})
			k.SettingsRow(c.Key("count"), uikit.SettingsRowProps{Label: "最大历史条目数", Description: "命中排除 tag 的条目不计入上限"}, nil, func() {
				value := strconv.Itoa(h.MaxItems)
				if k.Select(c.Key("select"), uikit.SelectProps{Label: "最大历史条目数", Value: &value, Items: cleanupChoices(h.MaxItems, []int{100, 500, 1000, 5000, 10000, 0}, " 条"), Disabled: disabled}).Changed {
					h.MaxItems, _ = strconv.Atoi(value)
					changed = true
				}
			})
			k.SettingsRow(c.Key("tags"), uikit.SettingsRowProps{Label: "不清理的 tag", Description: "命中任一所选 tag 时，保留记录及应用管理的文件"}, nil, func() {
				tags := slices.Clone(view.tags)
				for _, tag := range h.ExcludedTags {
					if !slices.Contains(tags, tag) {
						tags = append(tags, tag)
					}
				}
				sort.Strings(tags)
				ui.Column(c.Key("choices")).Gap(6).Children(func() {
					if len(tags) == 0 {
						k.Text(c, "暂无 tag。请先为历史记录添加 tag。", uikit.TextProps{})
					}
					for _, tag := range tags {
						selected := slices.Contains(h.ExcludedTags, tag)
						if k.Checkbox(c.Key(tag), &selected, uikit.CheckboxProps{Label: tag, Disabled: disabled}).Changed() {
							if selected {
								h.ExcludedTags = append(h.ExcludedTags, tag)
							} else {
								h.ExcludedTags = slices.DeleteFunc(h.ExcludedTags, func(t string) bool { return t == tag })
							}
							changed = true
						}
					}
				})
			})
		})
		k.SettingsGroup(c.Key("files"), uikit.SettingsGroupProps{Title: "文件存储"}, func() {
			k.SettingsRow(c.Key("quota"), uikit.SettingsRowProps{Label: "单设备缓存配额", Description: "仅限制默认文件缓存。无法清理时拒绝接收新文件。"}, nil, func() {
				quota := int(state.Config.FileCache.MaxBytes)
				value := strconv.Itoa(quota)
				items := []uikit.SelectItem{}
				values := []int{100 << 20, 500 << 20, 1 << 30, 2 << 30, 4 << 30, 8 << 30}
				if !slices.Contains(values, quota) {
					values = append(values, quota)
				}
				for _, n := range values {
					items = append(items, uikit.SelectItem{Value: strconv.Itoa(n), Label: strconv.Itoa(n/(1<<20)) + " MB"})
				}
				if k.Select(c.Key("select"), uikit.SelectProps{Label: "单设备缓存配额", Value: &value, Items: items, Disabled: view.busy}).Changed {
					n, _ := strconv.ParseInt(value, 10, 64)
					state.Config.FileCache.MaxBytes = config.Size(n)
					changed = true
				}
			})
		})
		k.Text(c.Key("note"), "关闭自动清理后，存储配额和显式过期仍生效。清理不会删除源文件或自定义接收目录中的文件。", uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
		if changed {
			g.saveCleanupSettings(state)
		}
	})
}
