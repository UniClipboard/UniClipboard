package desktopui

import (
	"path/filepath"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/uniclipboard/unic/app/desktop/boardui"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/desktopkeys"
	"github.com/uniclipboard/unic/plugin"
)

type qpEntry struct {
	highlights                         []engine.HistorySearchRange
	id, title, text, kind, source, age string
	tags                               []string
	favorite, lost                     bool
}
type qpFilter struct {
	dimension, value, label, matched string
	start, end                       int
	replace                          bool
}
type qpAction struct {
	id, label, shortcut string
	enabled             bool
}

// qpEffect is the complete simulated host boundary. It never calls OS services.
type qpEffect struct {
	Action   string
	Entry    string
	Target   string `json:",omitempty"`
	Plain    bool   `json:",omitempty"`
	KeepOpen bool   `json:",omitempty"`
}
type quickPanelState struct {
	colors       *boardui.ColorTheme
	previewImage ui.ImageSource
	language     string
	live         *quickLive
	richPreview  *richtext.Document
	pasteFailure string

	revealSelection      bool
	keySequence          shortcutSequence
	windowZoom, textZoom int

	blurDue                       time.Time
	nativeFocused                 func() bool
	imageDragging                 bool
	imageDragStart, imagePanStart ui.Point

	quit        func()
	relaxCursor int
	actualSize  bool
	imageID     string
	imagePan    ui.Point

	theme, mode, query, kind                       string
	favoriteOnly                                   bool
	queryStart, queryEnd                           int
	entries, rows                                  []qpEntry
	chips                                          []qpFilter
	selected, hovered, gridTop                     int
	visible, keyboard, suggestOpen, suggestFocused bool
	suggestCursor                                  int
	menu                                           string
	cursor                                         int
	previewID, pendingPreview, lastPreviewID       string
	expanded                                       bool
	satelliteScroll                                ui.ScrollState
	queryDue, previewDue                           time.Time
	now                                            time.Time
	effects                                        []qpEffect
	input                                          ui.Handle
	anchor                                         ui.Rect
	pointerX, pointerY                             float32
	pointerKnown                                   bool
	onUpdate                                       func()
	panelInvalidate, satelliteInvalidate           func()
	measureText                                    func(float32, ...ui.Span) (float32, float32)
	satelliteWidth, satelliteHeight                int
}

func (p *quickPanelState) update() {
	if p.panelInvalidate != nil {
		p.panelInvalidate()
	}
	if p.satelliteInvalidate != nil {
		p.satelliteInvalidate()
	}
	if p.onUpdate != nil {
		p.onUpdate()
	}
}
func (p *quickPanelState) hasActiveFilter() bool {
	return p.query != "" || p.kind != "" || len(p.chips) > 0 || p.favoriteOnly
}

func (p *quickPanelState) activePrefixWord() (searchWord, bool) {
	words := searchWords(p.query)
	if len(words) == 0 {
		return searchWord{}, false
	}
	if p.queryStart > 0 || p.queryEnd > 0 {
		for _, w := range words {
			if w.dimension != "" && w.start <= p.queryStart && w.end >= p.queryEnd {
				return w, true
			}
		}
	}
	for i := len(words) - 1; i >= 0; i-- {
		if words[i].dimension != "" && !words[i].committed {
			return words[i], true
		}
	}
	return searchWord{}, false
}

func (p *quickPanelState) literalQuery() string {
	if p.suggestOpen {
		if w, ok := p.activePrefixWord(); ok && (w.partial == "" || len(p.suggestions()) > 0) {
			r := []rune(p.query)
			return strings.TrimSpace(string(r[:w.start]) + string(r[w.end:]))
		}
	}
	return strings.TrimSpace(p.query)
}

func (p *quickPanelState) queryChanged() {
	p.suggestOpen = true
	if opts := p.suggestions(); len(opts) > 0 {
		p.suggestFocused = true
		p.suggestCursor = 0
	} else {
		p.suggestOpen = false
		p.suggestFocused = false
		p.suggestCursor = 0
	}
	p.hovered = -1
	p.keyboard = true
	p.menu = ""
	p.queryDue = p.now.Add(300 * time.Millisecond)
	p.update()
}
func (p *quickPanelState) tick(c *ui.Context) {
	if p.live != nil {
		p.live.d.live.bindWake(c)
		p.live.poll(p)
	}
	p.now = c.Now()
	if !p.blurDue.IsZero() {
		if p.now.Before(p.blurDue) {
			c.After(p.blurDue.Sub(p.now))
		} else {
			p.blurDue = time.Time{}
			if p.nativeFocused != nil && !p.nativeFocused() {
				if p.visible {
					if p.live != nil && p.live.system != nil {
						p.live.system.Dismiss(false)
					}
					p.visible = false
					p.update()
				}
			}
		}
	}
	if !p.queryDue.IsZero() {
		if !p.now.Before(p.queryDue) {
			p.queryDue = time.Time{}
			p.applySearch()
			p.update()
		} else {
			c.After(p.queryDue.Sub(p.now))
		}
	}
	if !p.previewDue.IsZero() {
		if !p.now.Before(p.previewDue) {
			p.previewDue = time.Time{}
			p.previewID = p.pendingPreview
			p.expanded = p.previewID != ""
			p.update()
		} else {
			c.After(p.previewDue.Sub(p.now))
		}
	}
}
func (p *quickPanelState) matches(e qpEntry, skip int) bool {
	kind := e.kind
	if kind == "代码" || kind == "链接" {
		kind = "文本"
	}

	q := strings.ToLower(p.literalQuery())
	if q != "" && !strings.Contains(strings.ToLower(e.title+" "+e.text), q) {
		return false
	}
	if p.favoriteOnly && !e.favorite {
		return false
	}
	groups := map[string][]string{}
	if p.kind != "" {
		groups["type"] = []string{p.kind}
	}
	for i, f := range p.chips {
		if i != skip {
			groups[f.dimension] = append(groups[f.dimension], f.value)
		}
	}
	for dim, values := range groups {
		if dim == "tag" {
			for _, v := range values {
				if !slices.Contains(e.tags, v) && !((v == "收藏" || v == plugin.TagFavorite) && e.favorite) {
					return false
				}
			}
			continue
		}
		matched := false
		for _, v := range values {
			switch dim {
			case "type":
				matched = matched || v == kind
			case "source":
				matched = matched || e.source == v
			case "time":
				matched = matched || v == "本周" || v == "本月" || v == "最近 7 天" || v == "最近 30 天" || (v == "今天" && e.age != "昨天") || (v == "昨天" && e.age == "昨天")
			case "extension":
				ext := strings.TrimPrefix(v, ".")
				for _, name := range append([]string{e.title}, strings.Split(e.text, "\n")...) {
					if strings.EqualFold(strings.TrimPrefix(filepath.Ext(name), "."), ext) {
						matched = true
						break
					}
				}
			}
		}
		if !matched {
			return false
		}
	}
	return true
}
func (p *quickPanelState) normalizeFilters() {
	order := map[string]int{"type": 0, "tag": 1, "source": 2, "time": 3, "extension": 4}
	slices.SortStableFunc(p.chips, func(a, b qpFilter) int { return order[a.dimension] - order[b.dimension] })
	count, at := 0, -1
	for i, f := range p.chips {
		if f.dimension == "type" {
			count++
			at = i
		}
	}
	if count == 1 && p.kind == "" {
		p.kind = p.chips[at].value
		p.chips = slices.Delete(p.chips, at, at+1)
	}
}
func (p *quickPanelState) applySearch() {
	if p.live != nil {
		p.rows = nil
		p.entries = nil
		// Reopening the same query can reuse an unchanged service revision.
		// Rebuild the cleared projection even when no new result serial arrives.
		p.live.seen = 0
		p.previewID = ""
		p.pendingPreview = ""
		p.selected = 0
		p.hovered = -1
		p.gridTop = 0
		p.revealSelection = true
		p.keyboard = true
		p.update()
		p.normalizeFilters()
		l := p.live.d.live
		l.invalidateSearch()
		l.loadHistory(p.live.d, false)
		return
	}
	p.normalizeFilters()
	p.relaxCursor = 0
	p.rows = nil
	p.revealSelection = true
	p.selected = 0
	p.hovered = -1
	p.gridTop = 0
	p.keyboard = true
	if p.mode == "history" {
		for _, e := range p.entries {
			if p.matches(e, -1) {
				p.rows = append(p.rows, e)
			}
		}
	}
	p.previewID = ""
	p.pendingPreview = ""
	p.previewDue = time.Time{}
	p.schedulePreview()
}
func (p *quickPanelState) active() *qpEntry {
	at := p.selected
	if p.hovered >= 0 && !p.keyboard {
		at = p.hovered
	}
	if at < 0 || at >= len(p.rows) {
		return nil
	}
	return &p.rows[at]
}
func (p *quickPanelState) previewEntry() *qpEntry {
	if p.menu != "" {
		return p.active()
	}
	if p.previewID == "" {
		return nil
	}
	for i := range p.entries {
		if p.entries[i].id == p.previewID {
			return &p.entries[i]
		}
	}
	return nil
}
func (p *quickPanelState) schedulePreview() {
	e := p.active()
	if e == nil {
		p.previewID = ""
		p.previewDue = time.Time{}
		return
	}
	if p.menu != "" || (p.previewID == e.id && p.previewDue.IsZero()) {
		return
	}
	if p.pendingPreview == e.id && !p.previewDue.IsZero() {
		return
	}
	p.pendingPreview = e.id
	delay := 500 * time.Millisecond
	if p.expanded {
		delay = 120 * time.Millisecond
	}
	p.previewDue = p.now.Add(delay)
}
func (p *quickPanelState) selectRow(at int) {
	if len(p.rows) == 0 {
		return
	}
	p.selected = max(0, min(at, len(p.rows)-1))
	p.revealSelection = true
	p.input.Focus()
	p.hovered = -1
	p.keyboard = true
	p.suggestFocused = false
	if p.kind == "图片" {
		row := p.selected / 3
		if row < p.gridTop {
			p.gridTop = row
		}
		if row >= p.gridTop+3 {
			p.gridTop = row - 2
		}
		p.gridTop = min(p.gridTop, max(0, (len(p.rows)+2)/3-3))
	}
	p.schedulePreview()
	p.update()
}
func (p *quickPanelState) suggestions() []qpFilter {
	if !p.suggestOpen || strings.TrimSpace(p.query) == "" {
		return nil
	}
	w, ok := p.activePrefixWord()
	if !ok {
		return nil
	}
	type qpCandidate struct {
		filter       qpFilter
		matchDim     string
		matchValue   string
		displayLabel string
		rawLabel     string
	}
	var pool []qpCandidate
	switch w.dimension {
	case "type":
		for _, item := range [][2]string{{"text", "文本"}, {"richtext", "富文本"}, {"image", "图片"}, {"file", "文件"}, {"folder", "文件夹"}} {
			pool = append(pool, qpCandidate{
				filter:       qpFilter{dimension: "type", value: item[1], label: "/" + item[1]},
				matchDim:     "type",
				matchValue:   item[0],
				displayLabel: p.tr(item[1]),
				rawLabel:     item[1],
			})
		}
	case "tag":
		if !p.favoriteOnly {
			pool = append(pool, qpCandidate{
				filter:       qpFilter{dimension: "favorite", value: plugin.TagFavorite, label: "★ " + p.tr("已收藏")},
				matchDim:     "favorite",
				matchValue:   plugin.TagFavorite,
				displayLabel: p.tr("已收藏"),
				rawLabel:     "已收藏",
			})
		}
		if p.live == nil {
			for _, item := range [][2]string{
				{plugin.TagLink, "链接"},
				{plugin.TagCode, "代码"},
				{plugin.TagImage, "图片"},
				{plugin.TagFiles, "文件"},
				{plugin.TagFolder, "文件夹"},
				{"工作", "工作"},
				{"开发", "开发"},
				{"设计", "设计"},
			} {
				val := item[1]
				pool = append(pool, qpCandidate{
					filter:       qpFilter{dimension: "tag", value: val, label: "#" + val},
					matchDim:     "tag",
					matchValue:   item[0],
					displayLabel: p.tr(val),
					rawLabel:     val,
				})
			}
		} else {
			seen := map[string]bool{}
			addTag := func(id string) {
				if id == "" || id == plugin.TagFavorite || seen[id] {
					return
				}
				seen[id] = true
				lbl := p.live.d.tagLabel(id)
				raw := lbl
				if t := p.live.d.tagByID(id); t != nil {
					raw = t.name
				}
				pool = append(pool, qpCandidate{
					filter:       qpFilter{dimension: "tag", value: id, label: "#" + lbl},
					matchDim:     "tag",
					matchValue:   id,
					displayLabel: lbl,
					rawLabel:     raw,
				})
			}
			for _, v := range p.live.d.live.catalogue["tag"] {
				addTag(v.Value)
			}
			for _, t := range p.live.d.tags {
				addTag(t.id)
			}
		}
	case "source":
		if p.live == nil {
			for _, v := range []string{"本机", "工作电脑", "Linux 工作站", "我的手机"} {
				matchVal := v
				if v == "本机" {
					matchVal = "local"
				}
				pool = append(pool, qpCandidate{
					filter:       qpFilter{dimension: "source", value: v, label: "@" + v},
					matchDim:     "source",
					matchValue:   matchVal,
					displayLabel: p.tr(v),
					rawLabel:     v,
				})
			}
		} else {
			seen := map[string]bool{}
			for _, v := range p.live.d.live.catalogue["source"] {
				if v.Value == "" || seen[v.Value] {
					continue
				}
				seen[v.Value] = true
				lbl := v.Label
				if lbl == "" {
					lbl = p.live.d.sourceName(v.Value)
				}
				pool = append(pool, qpCandidate{
					filter:       qpFilter{dimension: "source", value: v.Value, label: "@" + lbl},
					matchDim:     "source",
					matchValue:   v.Value,
					displayLabel: lbl,
					rawLabel:     lbl,
				})
			}
			for _, dev := range p.live.d.devices {
				if dev.id == "" || seen[dev.id] {
					continue
				}
				seen[dev.id] = true
				lbl := p.live.d.sourceName(dev.id)
				pool = append(pool, qpCandidate{
					filter:       qpFilter{dimension: "source", value: dev.id, label: "@" + lbl},
					matchDim:     "source",
					matchValue:   dev.id,
					displayLabel: lbl,
					rawLabel:     lbl,
				})
			}
		}
	case "time":
		for _, item := range [][2]string{
			{"today", "今天"},
			{"yesterday", "昨天"},
			{"day_before_yesterday", "前天"},
			{"last_7d", "最近 7 天"},
			{"last_30d", "最近 30 天"},
			{"this_week", "本周"},
			{"last_week", "上周"},
			{"this_month", "本月"},
			{"last_month", "上月"},
		} {
			pool = append(pool, qpCandidate{
				filter:       qpFilter{dimension: "time", value: item[1], label: item[1], replace: true},
				matchDim:     "time",
				matchValue:   item[0],
				displayLabel: p.tr(item[1]),
				rawLabel:     item[1],
			})
		}
	case "extension":
		if p.live == nil {
			for _, ext := range []string{"txt", "md", "jpg", "png", "pdf", "ts", "js", "json", "rs", "go"} {
				pool = append(pool, qpCandidate{
					filter:       qpFilter{dimension: "extension", value: ext, label: "." + ext, replace: true},
					matchDim:     "extension",
					matchValue:   ext,
					displayLabel: "." + ext,
					rawLabel:     ext,
				})
			}
		} else {
			for _, v := range p.live.d.live.catalogue["extension"] {
				if v.Value == "" {
					continue
				}
				pool = append(pool, qpCandidate{
					filter:       qpFilter{dimension: "extension", value: v.Value, label: "." + v.Value, replace: true},
					matchDim:     "extension",
					matchValue:   v.Value,
					displayLabel: "." + v.Value,
					rawLabel:     v.Value,
				})
			}
		}
	}

	type rankedFilter struct {
		f    qpFilter
		rank int
	}
	var ranked []rankedFilter
	for _, cand := range pool {
		f := cand.filter
		accepted := (f.dimension == "type" && p.kind == f.value) || (f.dimension == "favorite" && p.favoriteOnly)
		for _, old := range p.chips {
			if old.dimension == f.dimension && old.value == f.value {
				accepted = true
			}
		}
		if accepted {
			continue
		}
		rank, ok := matchSearchCandidate(cand.matchDim, cand.matchValue, cand.displayLabel, cand.rawLabel, w.partial)
		if !ok {
			continue
		}
		if w.dimension == "tag" && strings.TrimSpace(w.partial) == "" && f.dimension == "favorite" {
			rank = -1
		}
		f.matched = w.raw
		f.start = w.start
		f.end = w.end
		ranked = append(ranked, rankedFilter{f: f, rank: rank})
	}
	slices.SortStableFunc(ranked, func(a, b rankedFilter) int { return a.rank - b.rank })
	result := make([]qpFilter, len(ranked))
	for i, r := range ranked {
		result[i] = r.f
	}
	return result
}
func (p *quickPanelState) accept(f qpFilter) {
	if f.dimension == "favorite" {
		p.favoriteOnly = true
	} else {
		if f.replace {
			p.chips = slices.DeleteFunc(p.chips, func(old qpFilter) bool { return old.dimension == f.dimension })
			if f.dimension == "type" {
				p.kind = ""
			}
		}

		if f.dimension == "type" && p.kind == "" && !slices.ContainsFunc(p.chips, func(old qpFilter) bool { return old.dimension == "type" }) {
			p.kind = f.value
		} else {
			if f.dimension == "type" && p.kind != "" {
				p.chips = append(p.chips, qpFilter{dimension: "type", value: p.kind, label: "/" + p.kind})
				p.kind = ""
			}
			if f.dimension == "time" || f.dimension == "extension" {
				p.chips = slices.DeleteFunc(p.chips, func(old qpFilter) bool { return old.dimension == f.dimension })
			}
			p.chips = append(p.chips, f)
		}
	}
	runes := []rune(p.query)
	if f.end > f.start && f.end <= len(runes) {
		p.query = strings.TrimSpace(string(runes[:f.start]) + string(runes[f.end:]))
		p.queryStart = len([]rune(p.query))
		p.queryEnd = p.queryStart
	} else if at := strings.LastIndex(p.query, f.matched); at >= 0 {
		p.query = strings.TrimSpace(p.query[:at] + p.query[at+len(f.matched):])
		p.queryStart = len([]rune(p.query))
		p.queryEnd = p.queryStart
	}
	p.queryChanged()
	p.suggestOpen = false
	p.suggestFocused = false
}
func (p *quickPanelState) clear() {
	p.query = ""
	p.queryStart, p.queryEnd = 0, 0
	p.kind = ""
	p.favoriteOnly = false
	p.chips = nil
	p.menu = ""
	p.queryChanged()
	p.suggestOpen = false
	p.suggestFocused = false
}
func (p *quickPanelState) tab(reverse bool) {
	options := p.suggestions()
	if len(options) > 0 {
		if reverse {
			p.suggestFocused = true
			p.suggestCursor = (p.suggestCursor - 1 + len(options)) % len(options)
			p.update()
			return
		}
		at := 0
		if p.suggestFocused {
			at = min(p.suggestCursor, len(options)-1)
		}
		p.accept(options[at])
		return
	}
	types := []string{"", "文本", "富文本", "图片", "文件", "文件夹"}
	at := slices.Index(types, p.kind)
	step := 1
	if reverse {
		step = -1
	}
	p.chips = slices.DeleteFunc(p.chips, func(f qpFilter) bool { return f.dimension == "type" })
	p.kind = types[(at+step+len(types))%len(types)]
	p.queryChanged()
	p.suggestOpen = false
	p.suggestFocused = false
}
func (p *quickPanelState) restore(paste, plain, keep bool) {
	if p.live != nil {
		if p.live.d.live.busy {
			return
		}
		e := p.active()
		if e == nil || !p.queryDue.IsZero() {
			return
		}
		if plain && e.kind != "文本" && e.kind != "富文本" {
			return
		}
		if p.live.system == nil {
			return
		}
		p.pasteFailure = ""
		_ = p.live.system.Start(e.id, plain, paste, keep)
		p.menu = ""
		p.update()
		return
	}

	if !p.queryDue.IsZero() {
		return
	}
	e := p.active()
	if e == nil {
		return
	}
	if e.lost {
		return
	}
	action := "copy"
	if paste {
		action = "paste"
	}
	p.effects = append(p.effects, qpEffect{Action: action, Entry: e.id, Target: "TextEdit", Plain: plain, KeepOpen: keep})
	p.menu = ""
	if !keep {
		p.visible = false
	}
	p.update()
}
func (p *quickPanelState) actions() []qpAction {
	e := p.active()
	if p.live != nil {
		available := e != nil && !p.live.d.live.busy && (p.live.system == nil || !p.live.system.Busy)
		canPaste := available && p.live.system != nil && p.live.system.CanPaste
		favorite := "收藏"
		if e != nil && e.favorite {
			favorite = "取消收藏"
		}
		overrides := p.live.d.live.config.Config.Desktop.Shortcuts
		return []qpAction{
			{"paste", "粘贴到原应用", "⏎", canPaste},
			{"plain", "纯文本粘贴", "⇧⏎", canPaste && (e.kind == "文本" || e.kind == "富文本")},
			{"keep", "粘贴并保持面板", p.commandLabel() + "⏎", canPaste},
			{"copy", "复制到剪贴板", p.commandLabel() + "C", available},
			{"favorite", favorite, shortcutHint(overrides, desktopkeys.PanelFavorite), available},
			{"delete", "删除", p.commandLabel() + "⇧⌫", available},
			{"main", "打开主窗口", p.commandLabel() + "⇧O", true},
			{"settings", "设置…", p.commandLabel() + ",", true},
		}
	}
	if e == nil {
		return nil
	}
	usable := !e.lost
	if p.menu == "devices" {
		return []qpAction{{"send:all", "所有设备", "", true}, {"send:windows", "工作电脑", "", true}, {"send:linux", "Linux 工作站", "", false}, {"send:phone", "我的手机", "", true}}
	}
	rows := []qpAction{{"paste", "粘贴到 TextEdit", "⏎", usable}}
	if e.kind == "文本" || e.kind == "富文本" || e.kind == "代码" || e.kind == "链接" {
		rows = append(rows, qpAction{"plain", "粘贴为纯文本", "⇧⏎", usable})
	}
	rows = append(rows, qpAction{"keep", "粘贴并保持面板", p.commandLabel() + "⏎", usable}, qpAction{"copy", "只复制", p.commandLabel() + "C", usable})
	if e.kind == "文件" {
		rows = append(rows, qpAction{"paths", "粘贴文件路径", "", usable})
	}
	if e.kind == "链接" || e.kind == "文件" {
		rows = append(rows, qpAction{"open", "打开", p.commandLabel() + "O", true})
	}
	if e.kind == "文件" {
		rows = append(rows, qpAction{"reveal", "在文件夹中显示", "", true})
	}
	favorite := "收藏"
	if e.favorite {
		favorite = "取消收藏"
	}
	return append(rows, qpAction{"devices", "发送到设备", "", usable}, qpAction{"favorite", favorite, "", true}, qpAction{"main", "打开主窗口", p.commandLabel() + "⇧O", true}, qpAction{"delete", "删除", p.commandLabel() + "⇧⌫", true}, qpAction{"settings", "设置…", p.commandLabel() + ",", true})
}
func (p *quickPanelState) openActions() {
	p.input.Focus()
	if p.active() == nil || !p.queryDue.IsZero() {
		return
	}
	if p.menu != "" {
		p.menu = ""
		p.schedulePreview()
		p.update()
		return
	}
	p.menu = "main"
	p.expanded = true
	p.previewDue = time.Time{}
	p.cursor = 0
	for i, row := range p.actions() {
		if row.enabled {
			p.cursor = i
			break
		}
	}
	p.update()
}
func (p *quickPanelState) runAction(id string) {
	if p.live != nil {
		p.liveAction(id)
		return
	}
	e := p.active()
	if e == nil {
		return
	}
	if id == "devices" {
		p.menu = "devices"
		p.cursor = 0
		p.update()
		return
	}
	p.menu = ""
	switch id {
	case "paste":
		p.restore(true, false, false)
	case "plain":
		p.restore(true, true, false)
	case "keep":
		p.restore(true, false, true)
	case "copy":
		p.restore(false, false, false)
	case "favorite":
		for i := range p.entries {
			if p.entries[i].id == e.id {
				p.entries[i].favorite = !p.entries[i].favorite
			}
		}
		selected := p.selected
		p.applySearch()
		p.selectRow(selected)
	case "delete":
		selected := p.selected
		id := e.id
		p.entries = slices.DeleteFunc(p.entries, func(v qpEntry) bool { return v.id == id })
		p.applySearch()
		p.selectRow(selected)
	default:
		effect := qpEffect{Action: id, Entry: e.id}
		if strings.HasPrefix(id, "send:") {
			effect.Action = "send"
			effect.Target = strings.TrimPrefix(id, "send:")
		}
		p.effects = append(p.effects, effect)
		if id == "main" || id == "settings" || id == "paths" {
			p.visible = false
		}
	}
	p.schedulePreview()
	p.update()
}
func (p *quickPanelState) navigate(down bool) {
	if p.menu != "" {
		rows := p.actions()
		step := 1
		if !down {
			step = -1
		}
		for n := 0; n < len(rows); n++ {
			p.cursor = (p.cursor + step + len(rows)) % len(rows)
			if rows[p.cursor].enabled {
				break
			}
		}
		p.update()
		return
	}
	options := p.suggestions()
	if len(options) > 0 {
		p.suggestFocused = true
		step := 1
		if !down {
			step = -1
		}
		p.suggestCursor = (p.suggestCursor + step + len(options)) % len(options)
		p.update()
		return
	}
	if len(p.rows) == 0 {
		relaxOpts := p.relaxations()
		if len(relaxOpts) > 0 {
			step := 1
			if !down {
				step = -1
			}
			p.relaxCursor = (p.relaxCursor + step + len(relaxOpts)) % len(relaxOpts)
			p.update()
		}
		return
	}
	step := 1
	if p.kind == "图片" {
		step = 3
	}
	if !down {
		step = -step
	}
	p.selectRow(p.selected + step)
}

func (p *quickPanelState) pageNavigate(down bool) {
	if p.menu != "" || len(p.suggestions()) > 0 || len(p.rows) == 0 {
		return
	}
	step := 8
	if p.kind == "图片" {
		step = 9
	}
	if !down {
		step = -step
	}
	p.selectRow(p.selected + step)
}

func (p *quickPanelState) togglePreviewNow() {
	if p.menu != "" {
		return
	}
	if p.previewID != "" {
		p.previewID = ""
		p.pendingPreview = ""
		p.previewDue = time.Time{}
		p.expanded = false
		p.update()
		return
	}
	if e := p.active(); e != nil {
		p.previewID = e.id
		p.pendingPreview = e.id
		p.previewDue = time.Time{}
		p.expanded = true
		p.update()
	}
}

func (p *quickPanelState) scrollPreview(delta float32) {
	if p.menu != "" || p.previewEntry() == nil {
		return
	}
	next := max(float32(0), min(p.satelliteScroll.MaxY, p.satelliteScroll.Y+delta))
	p.satelliteScroll.ScrollTo(0, next, ui.Animation{Duration: 90 * time.Millisecond, Ease: ui.EaseOut})
	p.update()
}

func (p *quickPanelState) toggleImageActualSize() bool {
	e := p.previewEntry()
	if p.menu != "" || e == nil || !p.quickHasImagePreview(*e) {
		return false
	}
	p.actualSize = !p.actualSize
	p.imagePan = ui.Point{}
	p.imageDragging = false
	p.update()
	return true
}

func (p *quickPanelState) escape() {
	switch {
	case p.menu == "devices":
		p.menu = "main"
		p.cursor = 0
	case p.menu != "":
		p.menu = ""
		p.schedulePreview()
	case len(p.suggestions()) > 0:
		p.suggestOpen = false
		p.suggestFocused = false
	case p.hasActiveFilter():
		p.clear()
	default:
		if p.live != nil && p.live.system != nil {
			p.live.system.Dismiss(true)
		}
		p.visible = false
	}
	p.update()
}

// Relax each dimension independently. Show only proposals with matches.
type qpRelaxation struct {
	dimension, label string
	count            int
}

func (p *quickPanelState) relaxations() []qpRelaxation {
	if p.live != nil {
		return p.liveRelaxations()
	}
	result := []qpRelaxation{}
	for _, dim := range []string{"favorite", "type", "tag", "time", "source", "extension"} {
		values := []string{}
		if dim == "favorite" {
			if !p.favoriteOnly {
				continue
			}
			values = []string{plugin.TagFavorite}
		} else {
			if dim == "type" && p.kind != "" {
				values = append(values, p.kind)
			}
			for _, f := range p.chips {
				if f.dimension == dim {
					values = append(values, f.value)
				}
			}
			if len(values) == 0 {
				continue
			}
		}
		label := ""
		switch dim {
		case "favorite":
			label = p.tr("去掉 ★ 已收藏")
		case "type":
			label = p.tr("去掉所有类型")
			if len(values) == 1 {
				label = p.tr("去掉 /") + values[0]
			}
		case "tag":
			label = p.tr("去掉所有标签")
			if len(values) == 1 {
				label = p.tr("去掉 #") + values[0]
			}
		case "time":
			label = p.tr("时间范围扩大到全部")
		case "source":
			label = p.tr("包含所有设备")
			if len(values) == 1 {
				label = p.tr("去掉 @") + values[0] + p.tr("，包含所有设备")
			}
		case "extension":
			label = p.tr("去掉扩展名限制")
		}
		// Copy only the fixture model used by matching. Never copy native callbacks.
		relaxed := quickPanelState{kind: p.kind, query: p.query, favoriteOnly: p.favoriteOnly}
		if dim == "favorite" {
			relaxed.favoriteOnly = false
		}
		if dim == "type" {
			relaxed.kind = ""
		}
		for _, f := range p.chips {
			if f.dimension != dim {
				relaxed.chips = append(relaxed.chips, f)
			}
		}
		count := 0
		for _, e := range p.entries {
			if relaxed.matches(e, -1) {
				count++
			}
		}
		if count > 0 {
			result = append(result, qpRelaxation{dim, label, count})
		}
	}
	if len(result) > 3 {
		result = result[:3]
	}
	return result
}
func (p *quickPanelState) relax(dim string) {
	if dim == "favorite" {
		p.favoriteOnly = false
	}
	if dim == "type" {
		p.kind = ""
	}
	p.chips = slices.DeleteFunc(p.chips, func(f qpFilter) bool { return f.dimension == dim })
	p.queryChanged()
	p.suggestOpen = false
	p.suggestFocused = false
}

func (p *quickPanelState) host(action string) {
	if p.live != nil {
		if p.live.system != nil {
			p.live.system.Dismiss(false)
		}
		if p.live.root != nil {
			p.live.root(action)
		}
		p.visible = false
		p.update()
		return
	}
	effect := qpEffect{Action: action}
	if e := p.active(); e != nil {
		effect.Entry = e.id
	}
	p.effects = append(p.effects, effect)
	p.visible = false
	p.menu = ""
	p.update()
}

func (p *quickPanelState) toggleFavoriteFilter() {
	p.favoriteOnly = !p.favoriteOnly
	p.menu = ""
	p.suggestOpen, p.suggestFocused = false, false
	p.queryDue = time.Time{}
	p.applySearch()
	p.update()
}

func (p *quickPanelState) setFavoriteFilter(enabled bool) {
	p.chips = slices.DeleteFunc(p.chips, func(f qpFilter) bool { return f.dimension == "tag" && f.value == plugin.TagFavorite })
	p.favoriteOnly = enabled
	if enabled {
		p.query, p.kind, p.menu = "", "", ""
		p.queryStart, p.queryEnd = 0, 0
		p.chips = nil
		p.queryDue = time.Time{}
		p.suggestOpen, p.suggestFocused = false, false
	}
}
