package desktopui

import (
	"context"
	"slices"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"
)

func favoriteHistoryQuery() engine.HistorySearchQuery {
	return engine.HistorySearchQuery{Tags: []string{plugin.TagFavorite}}
}

func withFavoriteTag(tags []string, on bool) []string {
	tags = slices.DeleteFunc(slices.Clone(tags), func(tag string) bool { return tag == plugin.TagFavorite })
	if on {
		tags = append(tags, plugin.TagFavorite)
	}
	return tags
}

func (d *viewState) allFavorited(ids []string) bool {
	for _, id := range ids {
		if d.live != nil {
			if !slices.Contains(d.live.items[id].Tags, plugin.TagFavorite) {
				return false
			}
			continue
		}
		if !slices.ContainsFunc(d.entries, func(e entry) bool { return e.id == id && e.pinned }) {
			return false
		}
	}
	return len(ids) > 0
}

func (d *viewState) togglePin() {
	d.toggleFavorites(d.list.Selection())
}

func (d *viewState) toggleFavorites(ids []string) {
	if len(ids) == 0 {
		return
	}
	on := !d.allFavorited(ids)
	if d.live == nil {
		for i := range d.entries {
			if slices.Contains(ids, d.entries[i].id) {
				d.entries[i].pinned = on
			}
		}
		return
	}
	if d.live.busy {
		return
	}
	records := make([]api.HistoryItem, 0, len(ids))
	for _, id := range ids {
		it, ok := d.live.items[id]
		if !ok {
			d.live.message = "历史已变化。请刷新后重试。"
			return
		}
		if slices.Contains(it.Tags, plugin.TagFavorite) != on {
			records = append(records, it)
		}
	}
	client := d.live.client
	success := d.tr("取消收藏")
	if on {
		success = d.tr("已收藏")
	}
	d.mutate(func(ctx context.Context) error {
		for _, it := range records {
			tags := withFavoriteTag(it.Tags, on)
			if _, err := client.UpdateHistory(ctx, it.ID, api.HistoryUpdate{Revision: it.Revision, Tags: &tags}); err != nil {
				return err
			}
		}
		return nil
	}, success)
}
