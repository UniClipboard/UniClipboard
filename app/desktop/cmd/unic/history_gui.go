package main

import (
	"context"
	"fmt"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
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
	t := c.Theme()
	ui.Row(c).Gap(8).Children(func() {
		ui.TextInput(c.Key("history-tag"), &g.historyTag).Placeholder("按标签筛选").Grow(1)
		if ui.Button(c.Key("history-filter"), "筛选").Disabled(s.historyBusy).Clicked() {
			g.loadHistory("", g.historyTag)
		}
		if ui.Button(c.Key("history-refresh"), "最新").Disabled(s.historyBusy).Clicked() {
			g.loadHistory("", g.historyTag)
		}
	})
	if s.historyError != "" {
		ui.Text(c, s.historyError).TextColor(t.Warning)
	}
	if s.historyBusy {
		ui.Spinner(c)
	}
	if len(s.history) == 0 && !s.historyBusy {
		ui.Text(c, "暂无历史。迁移完成并重新启动 unicd 后会显示在这里。").TextColor(t.TextMuted)
	}
	for _, item := range s.history {
		ui.Column(c.Key(item.ID)).Gap(6).Padding(12).Radius(10).Children(func() {
			ui.Text(c, s.historyPreviews[item.ID]).FontSize(13)
			ui.Row(c).Gap(6).Children(func() {
				date := time.UnixMilli(item.CreatedAt).Format("2006-01-02 15:04")
				if item.Origin == "migration" {
					date += " · 从 v1 导入"
				}
				ui.Text(c, date).FontSize(11).TextColor(t.TextMuted)
				ui.Spacer(c)
				if ui.Button(c.Key("use"), "使用").Clicked() {
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
				ui.Text(c, fmt.Sprint(item.Tags)).FontSize(11).TextColor(t.TextMuted)
			}
		})
		ui.Divider(c)
	}
	if s.historyCursor != "" {
		if ui.Button(c.Key("history-next"), "更早的历史").Disabled(s.historyBusy).Clicked() {
			g.loadHistory(s.historyCursor, g.historyTag)
		}
	}
}
