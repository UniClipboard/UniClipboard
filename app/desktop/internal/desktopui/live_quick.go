package desktopui

import (
	"context"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/quickpanel"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type quickLive struct {
	system            *quickpanel.Controller
	d                 *viewState
	seen, contentSeen uint64
	resumeSelection   string
	root              func(string)
}

func newLiveQuick(ctx context.Context, client *api.Client, theme string, root func(string)) *quickPanelState {
	p := &quickPanelState{theme: theme, mode: "history", visible: true, hovered: -1, keyboard: true}
	desktop := NewDesktop(ctx, client)
	p.live = &quickLive{d: desktop.d, root: root}
	p.live.d.live.queryOverride = func() engine.HistorySearchQuery { return p.searchQuery() }
	return p
}
func (p *quickPanelState) searchQuery() engine.HistorySearchQuery {
	q := engine.HistorySearchQuery{TextTerms: queryLiteral(p.query), TagMatch: "any"}
	add := func(dim, value string) {
		switch dim {
		case "type":
			m := map[string]engine.HistorySearchType{"文本": "text", "富文本": "richtext", "图片": "image", "文件": "files"}
			if t := m[value]; t != "" {
				q.Types = append(q.Types, t)
			}
		case "tag":
			q.Tags = append(q.Tags, value)
		case "source":
			q.Sources = append(q.Sources, searchSource(value))
		case "time":
			d := viewState{timeRange: map[string]string{"今天": "today", "昨天": "yesterday", "前天": "day_before_yesterday", "本周": "this_week", "上周": "last_week", "本月": "this_month", "上月": "last_month"}[value]}
			t := d.liveQuery()
			q.CreatedFromMs, q.CreatedBeforeMs = t.FromMs, t.BeforeMs
		}
	}
	if p.kind != "" {
		add("type", p.kind)
	}
	for _, f := range p.chips {
		add(f.dimension, f.value)
	}
	return q
}
func (q *quickLive) poll(p *quickPanelState) {
	d := q.d
	l := d.live
	// The existing 300 ms editor debounce remains authoritative.
	if p.queryDue.IsZero() {
		d.search.buffer = p.query
		d.search.start = len([]rune(p.query))
		d.search.end = d.search.start
		if p.previewID != "" {
			d.selected = p.previewID
		}
		l.poll(d)
	}
	if q.seen != l.appliedSerial || q.contentSeen != l.contentSerial {
		resultsChanged := q.seen != l.appliedSerial
		q.seen, q.contentSeen = l.appliedSerial, l.contentSerial
		old := ""
		if a := p.active(); a != nil {
			old = a.id
		} else if resultsChanged {
			old = q.resumeSelection
		}
		if resultsChanged {
			q.resumeSelection = ""
		}
		p.entries = nil
		for _, v := range d.entries {
			kind := kindName(v.kind)
			for _, part := range l.items[v.id].Parts {
				if v.kind == 0 && part.MIME == "text/html" {
					kind = "富文本"
				}
			}
			text := v.text
			if text == "" {
				text = v.preview
			}
			p.entries = append(p.entries, qpEntry{highlights: v.highlights, id: v.id, title: d.entryTitle(v), text: text, kind: kind, source: d.sourceName(v.source), age: d.displayDate(v.date), tags: slices.Clone(v.tags), favorite: v.pinned, lost: v.contentError != ""})
		}
		p.rows = slices.Clone(p.entries)
		p.selected = 0
		for i, v := range p.rows {
			if v.id == old {
				p.selected = i
				break
			}
		}
		if resultsChanged {
			p.hovered = -1
			p.schedulePreview()
		}
		p.update()
	}

	if p.queryDue.IsZero() && !l.loading && !time.Now().Before(l.pageRetry) && l.cursor != "" && (p.selected >= len(p.rows)-8 || p.hovered >= len(p.rows)-8) {
		l.loadHistory(d, true)
	}
}
func (p *quickPanelState) quickImageSize(e qpEntry) (float32, float32) {
	if p.live != nil {
		for _, v := range p.live.d.entries {
			if v.id == e.id {
				return imageDimensions(v)
			}
		}
	}
	return ui.ImageSize(p.previewImage)
}

type quickImage struct {
	source  ui.ImageSource
	history *historyImage
}

func (image quickImage) element(c *ui.Context) ui.Element {
	element := ui.Image(c, image.source)
	if image.history != nil && !image.history.released {
		return element.DrawOver(image.history.afterAcceleratedPaint)
	}
	return element
}

func (p *quickPanelState) quickImage(e qpEntry) quickImage {
	if p.live == nil {
		if e.kind == "图片" || qpIsSingleImageFile(e) {
			return quickImage{source: p.previewImage}
		}
		return quickImage{}
	}
	for _, v := range p.live.d.entries {
		if v.id == e.id {
			if v.picture != nil {
				return quickImage{source: v.picture.bitmap, history: v.picture}
			}
			return quickImage{source: v.image}
		}
	}
	return quickImage{}
}
func (p *quickPanelState) quickImageAvailable(e qpEntry) bool {
	if p.live == nil {
		return p.previewImage != nil && (e.kind == "图片" || qpIsSingleImageFile(e))
	}
	for _, v := range p.live.d.entries {
		if v.id == e.id {
			return v.picture != nil || v.image != nil
		}
	}
	return false
}
func (p *quickPanelState) quickImageLoading(e qpEntry) bool {
	if p.live == nil || e.kind != "文件" || e.lost {
		return false
	}
	l := p.live.d.live
	if _, cached := l.details[e.id]; cached {
		return false
	}
	for _, v := range p.live.d.entries {
		if v.id == e.id {
			return p.live.d.expectImagePreview(v)
		}
	}
	return false
}
func (p *quickPanelState) quickHasImagePreview(e qpEntry) bool {
	return e.kind == "图片" || (e.kind == "文件" && (p.quickImageAvailable(e) || p.quickImageLoading(e)))
}
func (p *quickPanelState) liveRelaxations() []qpRelaxation {
	q := p.searchQuery()
	queries := []engine.HistorySearchQuery{}
	dims := []string{}
	add := func(dim string, on bool, edit func(*engine.HistorySearchQuery)) {
		if !on {
			return
		}
		trial := copySearchQuery(q)
		edit(&trial)
		dims = append(dims, dim)
		queries = append(queries, trial)
	}
	add("type", len(q.Types) > 0, func(q *engine.HistorySearchQuery) { q.Types = nil })
	add("tag", len(q.Tags) > 0, func(q *engine.HistorySearchQuery) { q.Tags = nil })
	add("time", q.CreatedFromMs > 0 || q.CreatedBeforeMs > 0, func(q *engine.HistorySearchQuery) { q.CreatedFromMs = 0; q.CreatedBeforeMs = 0 })
	add("source", len(q.Sources) > 0, func(q *engine.HistorySearchQuery) { q.Sources = nil })
	l := p.live.d.live
	l.requestCounts(queries)
	out := []qpRelaxation{}
	labels := map[string]string{"type": "去掉所有类型", "tag": "去掉所有标签", "time": "时间范围扩大到全部", "source": "包含所有设备"}
	for i, q := range queries {
		if n, ok := l.counts[searchQueryKey(q)]; ok && n > 0 {
			out = append(out, qpRelaxation{dimension: dims[i], label: labels[dims[i]], count: int(n)})
		}
	}
	if len(out) > 3 {
		out = out[:3]
	}
	return out
}
func (p *quickPanelState) liveAction(id string) {
	if id == "main" || id == "settings" {
		p.host(id)
		return
	}
	e := p.active()
	if e == nil {
		return
	}
	d := p.live.d
	if p.live.system != nil && p.live.system.Busy {
		return
	}
	switch id {
	case "paste":
		p.restore(true, false, false)
	case "plain":
		p.restore(true, true, false)
	case "copy":
		p.restore(false, false, false)
	case "delete":
		d.deleteLiveEntries([]string{e.id})
	case "favorite":
		d.toggleFavorites([]string{e.id})
	}
	p.menu = ""
	p.update()
}

// OpenQuickPanel uses the same target capture path for tray and global activation.
func (s *Desktop) OpenQuickPanel() {
	if s.d.openQuickPanel != nil {
		s.d.openQuickPanel()
	}
}
