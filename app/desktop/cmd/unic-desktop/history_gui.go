package main

import (
	"context"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

func (g *gui) loadHistory(cursor, tag string) {
	g.mu.Lock()
	if g.historyBusy {
		g.mu.Unlock()
		return
	}
	g.historyBusy = true
	g.mu.Unlock()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		page, err := g.c.ListHistory(ctx, cursor, tag)
		previews := map[string]string{}
		if err == nil {
			for _, item := range page.Items {
				preview, previewErr := g.c.HistoryPreview(ctx, item)
				if previewErr == nil {
					if utf8.RuneCountInString(preview) > 160 {
						preview = string([]rune(preview)[:160]) + "…"
					}
					previews[item.ID] = preview
				}
			}
		}
		g.mu.Lock()
		g.historyBusy = false
		if err != nil {
			g.historyError = "读取历史失败，请重试"
		} else {
			g.historyLoaded = true
			g.historyError = ""
			g.history = page.Items
			g.historyCursor = page.NextCursor
			g.historyPreviews = previews
		}
		g.mu.Unlock()
		g.win.Invalidate()
	}()
}
func (g *gui) historyTab(c *ui.Context, s snapshot) {
	k := uikit.For(c.Theme().Dark)
	t := c.Theme()
	ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
		field := k.TextField(c.Key("history-tag"), uikit.TextFieldProps{
			Label: "按标签筛选", Value: &g.historyTag, Placeholder: "按标签筛选",
		})
		field.Element.Grow(1)
		if k.Button(c.Key("history-filter"), "筛选", uikit.ButtonProps{Disabled: s.historyBusy}).Clicked() || (field.Submitted && !s.historyBusy) {
			g.loadHistory("", g.historyTag)
		}
		if k.Button(c.Key("history-refresh"), "最新", uikit.ButtonProps{Disabled: s.historyBusy}).Clicked() {
			g.loadHistory("", g.historyTag)
		}
	})
	if s.historyError != "" {
		k.Text(c, s.historyError, uikit.TextProps{}).TextColor(t.Warning)
	}
	if s.historyBusy {
		ui.Spinner(c)
	}
	if len(s.history) == 0 && !s.historyBusy {
		k.ContentUnavailable(c, uikit.ContentUnavailableProps{
			Icon: uikit.IconHistory, Title: "暂无历史",
			Description: "复制文本、图片或文件后，历史会显示在这里。",
		}, nil)
	}
	if len(s.history) == 0 {
		return
	}
	k.Scroll(c).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).Gap(10).Padding(4).Children(func() {
			for _, item := range s.history {
				k.SettingsGroup(c.Key(item.ID), uikit.SettingsGroupProps{}, func() {
					ui.Column(c).Gap(6).Padding(12).Children(func() {
						k.Text(c, s.historyPreviews[item.ID], uikit.TextProps{}).FontSize(13)
						ui.Row(c).Gap(6).Children(func() {
							date := time.UnixMilli(item.CreatedAt).Format("2006-01-02 15:04")
							if item.Origin == "migration" {
								date += " · 从 v1 导入"
							}
							k.Text(c, date, uikit.TextProps{}).FontSize(11).TextColor(t.TextMuted)
							ui.Spacer(c)
							if k.Button(c.Key("use"), "使用", uikit.ButtonProps{}).Clicked() {
								id := item.ID
								go func() {
									ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
									defer cancel()
									err := g.c.UseHistory(ctx, id)
									g.mu.Lock()
									if err != nil {
										g.historyError = "无法使用此条历史"
									} else {
										g.historyError = ""
									}
									g.mu.Unlock()
									g.win.Invalidate()
								}()
							}
						})
						if len(item.Tags) > 0 {
							ui.Row(c).Gap(6).Wrap().Children(func() {
								for _, tag := range item.Tags {
									k.Tag(c.Key(tag), tag, uikit.TagProps{})
								}
							})
						}
					})
				})
			}
			if s.historyCursor != "" {
				if k.Button(c.Key("history-next"), "更早的历史", uikit.ButtonProps{Disabled: s.historyBusy}).Clicked() {
					g.loadHistory(s.historyCursor, g.historyTag)
				}
			}
		})
	})
}
