package desktopdemo

import (
	"fmt"
	"slices"
	"strings"
	"time"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/uniclipboard/unic/engine"
)

const quickPanelReference = "885546c7cf6f6f0d2076104efd1d90d9f73d9744"

type qpEntry struct {
	highlights                         []engine.HistorySearchRange
	id, title, text, kind, source, age string
	tags                               []string
	favorite, lost                     bool
}
type qpFilter struct {
	dimension, value, label, matched string
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
type quickDemo struct {
	language    string
	live        *quickLive
	richPreview *richtext.Document

	revealSelection bool

	blurDue                       time.Time
	nativeFocused                 func() bool
	imageDragging                 bool
	imageDragStart, imagePanStart ui.Point

	quit        func()
	relaxCursor int
	actualSize  bool
	imageID     string
	imagePan    ui.Point

	theme, mode, query, kind, message              string
	entries, rows                                  []qpEntry
	chips                                          []qpFilter
	selected, hovered, gridTop                     int
	visible, keyboard, suggestOpen, suggestFocused bool
	suggestCursor                                  int
	menu                                           string
	cursor                                         int
	previewID, pendingPreview                      string
	expanded                                       bool
	queryDue, previewDue                           time.Time
	now                                            time.Time
	effects                                        []qpEffect
	input                                          ui.Handle
	anchor                                         ui.Rect
	pointerX, pointerY                             float32
	pointerKnown                                   bool
	onUpdate                                       func()
	panelInvalidate, satelliteInvalidate           func()
}

func newQuickDemo(theme, mode string) *quickDemo {
	p := &quickDemo{theme: theme, mode: mode, visible: true, hovered: -1, keyboard: true}
	p.entries = []qpEntry{
		{id: "note", title: "周一产品讨论", text: "周一产品讨论\n\n1. 确认桌面端布局与导航。\n2. 核对设备连接状态。\n3. 整理本周设计评审记录。\n\n下次评审：周三 14:00。", kind: "文本", source: "本机", age: "刚刚", tags: []string{"工作"}},
		{id: "code", title: "HTTP 服务示例", text: "func main() {\n    http.HandleFunc(\"/health\", handler)\n    http.ListenAndServe(\":8080\", nil)\n}", kind: "代码", source: "Linux 工作站", age: "2分钟前", tags: []string{"代码", "开发"}},
		{id: "link", title: "github.com/UniClipboard/UniClipboard", text: "https://github.com/UniClipboard/UniClipboard", kind: "链接", source: "工作电脑", age: "5分钟前", tags: []string{"链接", "工作"}},
		{id: "rich", title: "设计评审记录", text: "设计评审记录\n\n确认列表高度与键盘操作。\n保留清晰的选中状态。", kind: "富文本", source: "本机", age: "8分钟前", tags: []string{"工作"}},
		{id: "files", title: "界面规范.pdf", text: "界面规范.pdf\n组件清单.csv\n桌面布局.png", kind: "文件", source: "工作电脑", age: "12分钟前", tags: []string{"设计", "工作"}},
		{id: "image", title: "桌面布局.png", text: "桌面布局.png", kind: "图片", source: "我的手机", age: "16分钟前", tags: []string{"设计"}},
		{id: "schedule", title: "下午设计评审：确认三栏布局、字体和控件间距", text: "下午 14:00 开始设计评审。请准备界面规范与组件清单。", kind: "文本", source: "本机", age: "20分钟前"},
		{id: "meeting", title: "会议室预订", text: "已预订 3 楼会议室。\n时间：14:00—15:00。\n参会人数：6 人。", kind: "文本", source: "本机", age: "25分钟前", favorite: true},
		{id: "older", title: "发布检查清单", text: "确认版本号\n确认安装包\n核对更新说明", kind: "文本", source: "本机", age: "昨天", tags: []string{"开发"}},
		{id: "lost", title: "丢失的文件.zip", text: "丢失的文件.zip", kind: "文件", source: "工作电脑", age: "1小时前", lost: true},
	}
	for i := 1; i <= 14; i++ {
		p.entries = append(p.entries, qpEntry{id: fmt.Sprintf("image-%02d", i), title: fmt.Sprintf("界面方案-%02d.png", i), text: fmt.Sprintf("界面方案-%02d.png", i), kind: "图片", source: "我的手机", age: "昨天", tags: []string{"设计"}})
	}
	p.applySearch()
	return p
}
func (p *quickDemo) update() {
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
func (p *quickDemo) queryChanged() {
	p.suggestOpen = true
	p.suggestFocused = false
	p.suggestCursor = 0
	p.hovered = -1
	p.keyboard = true
	p.menu = ""
	p.message = ""
	p.queryDue = p.now.Add(300 * time.Millisecond)
	p.update()
}
func (p *quickDemo) tick(c *ui.Context) {
	if p.live != nil {
		p.live.poll(p)
		c.After(200 * time.Millisecond)
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
func (p *quickDemo) matches(e qpEntry, skip int) bool {
	kind := e.kind
	if kind == "代码" || kind == "链接" {
		kind = "文本"
	}

	q := strings.TrimSpace(strings.ToLower(p.query))
	// Prefix-only and punctuation-only input has no literal search term.
	if strings.IndexFunc(q, unicode.IsLetter) < 0 && strings.IndexFunc(q, unicode.IsNumber) < 0 {
		q = ""
	}
	if q != "" && !strings.Contains(strings.ToLower(e.title+" "+e.text), q) {
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
		matched := false
		for _, v := range values {
			switch dim {
			case "type":
				matched = matched || v == kind
			case "tag":
				matched = matched || slices.Contains(e.tags, v) || (v == "收藏" && e.favorite)
			case "source":
				matched = matched || e.source == v
			case "time":
				matched = matched || v == "本周" || v == "本月" || (v == "今天" && e.age != "昨天") || (v == "昨天" && e.age == "昨天")
			}
		}
		if !matched {
			return false
		}
	}
	return true
}
func (p *quickDemo) normalizeFilters() {
	order := map[string]int{"type": 0, "tag": 1, "source": 2, "time": 3}
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
func (p *quickDemo) applySearch() {
	if p.live != nil {
		p.rows = nil
		p.entries = nil
		p.previewID = ""
		p.pendingPreview = ""
		p.selected = 0
		p.hovered = -1
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
func (p *quickDemo) active() *qpEntry {
	at := p.selected
	if p.hovered >= 0 && !p.keyboard {
		at = p.hovered
	}
	if at < 0 || at >= len(p.rows) {
		return nil
	}
	return &p.rows[at]
}
func (p *quickDemo) schedulePreview() {
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
func (p *quickDemo) selectRow(at int) {
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
func (p *quickDemo) suggestions() []qpFilter {
	if !p.suggestOpen || strings.TrimSpace(p.query) == "" {
		return nil
	}
	types := []string{"文本", "富文本", "图片", "文件"}
	tags := []string{"链接", "代码", "收藏", "图片", "文件夹", "工作", "开发", "设计"}
	sources := []string{"本机", "工作电脑", "Linux 工作站", "我的手机"}
	times := []string{"今天", "昨天", "前天", "本周", "上周", "本月", "上月"}
	all := []qpFilter{}
	add := func(dim, prefix string, values []string) {
		for _, v := range values {
			all = append(all, qpFilter{dimension: dim, value: v, label: prefix + v})
		}
	}
	add("type", "/", types)
	if p.live == nil {
		add("tag", "#", tags)
		add("source", "@", sources)
	} else {
		for _, v := range p.live.d.live.catalogue["tag"] {
			all = append(all, qpFilter{dimension: "tag", value: v.Value, label: "#" + p.live.d.tagLabel(v.Value)})
		}
		for _, v := range p.live.d.live.catalogue["source"] {
			all = append(all, qpFilter{dimension: "source", value: v.Value, label: "@" + v.Label})
		}
	}
	add("time", "", times)
	initials := map[string]string{"wb": "文本", "fwb": "富文本", "tp": "图片", "wj": "文件", "lj": "链接", "dm": "代码", "sc": "收藏", "wjj": "文件夹", "jt": "今天", "zt": "昨天", "bz": "本周", "sz": "上周", "by": "本月", "sy": "上月"}
	words := strings.Fields(p.query)
	aliases := map[string]string{"文本": "text", "富文本": "richtext", "图片": "image", "文件": "file", "链接": "link", "代码": "code", "收藏": "favorited", "文件夹": "directory"}
	result := []qpFilter{}
	for _, f := range all {
		accepted := p.kind == f.value && f.dimension == "type"
		for _, old := range p.chips {
			if old.dimension == f.dimension && old.value == f.value {
				accepted = true
			}
		}
		if accepted {
			continue
		}
		for _, token := range words {
			partial, dim := token, ""
			if strings.HasPrefix(token, "/") {
				dim = "type"
				partial = token[1:]
			}
			if strings.HasPrefix(token, "#") {
				dim = "tag"
				partial = token[1:]
			}
			if strings.HasPrefix(token, "@") {
				dim = "source"
				partial = token[1:]
			}
			if dim != "" && f.dimension != dim {
				continue
			}
			// Device names require @, unless their known fixture initials are typed.
			sourceInitials := map[string]string{"本机": "bj", "工作电脑": "gzdn", "Linux 工作站": "linux", "我的手机": "wdsj"}
			sourceMatch := f.dimension == "source" && dim == "" && len(partial) >= 2 && strings.HasPrefix(sourceInitials[f.value], strings.ToLower(partial))
			if f.dimension == "source" && dim == "" && !sourceMatch {
				continue
			}
			aliasMatch := aliases[f.value] != "" && strings.Contains(aliases[f.value], strings.ToLower(partial))
			if partial == "" || (strings.Contains(strings.ToLower(f.value), strings.ToLower(partial)) || (p.live != nil && strings.Contains(strings.ToLower(f.label), strings.ToLower(partial)))) || aliasMatch || f.value == initials[partial] || sourceMatch {
				f.matched = token
				f.replace = f.value == initials[partial] || sourceMatch || f.dimension == "time"
				result = append(result, f)
				break
			}
		}
	}

	return result
}
func (p *quickDemo) accept(f qpFilter) {
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
		if f.dimension == "time" {
			p.chips = slices.DeleteFunc(p.chips, func(old qpFilter) bool { return old.dimension == "time" })
		}
		p.chips = append(p.chips, f)
	}
	if at := strings.LastIndex(p.query, f.matched); at >= 0 {
		p.query = strings.TrimSpace(p.query[:at] + p.query[at+len(f.matched):])
	}
	p.queryChanged()
	p.suggestOpen = false
}
func (p *quickDemo) clear() {
	p.query = ""
	p.kind = ""
	p.chips = nil
	p.menu = ""
	p.message = ""
	p.queryChanged()
	p.suggestOpen = false
}
func (p *quickDemo) tab(reverse bool) {
	options := p.suggestions()
	if !reverse && len(options) > 0 {
		at := 0
		if p.suggestFocused {
			at = min(p.suggestCursor, len(options)-1)
		}
		p.accept(options[at])
		return
	}
	types := []string{"", "文本", "富文本", "图片", "文件"}
	at := slices.Index(types, p.kind)
	step := 1
	if reverse {
		step = -1
	}
	p.chips = slices.DeleteFunc(p.chips, func(f qpFilter) bool { return f.dimension == "type" })
	p.kind = types[(at+step+len(types))%len(types)]
	p.queryChanged()
	p.suggestOpen = false
}
func (p *quickDemo) restore(paste, plain, keep bool) {
	if p.live != nil {
		if p.live.d.live.busy {
			return
		}
		e := p.active()
		if e == nil || !p.queryDue.IsZero() {
			return
		}
		if plain && e.kind != "文本" && e.kind != "富文本" {
			p.message = "此内容不支持纯文本粘贴。"
			p.update()
			return
		}
		if p.live.system == nil {
			p.message = "系统交互尚未连接。"
			p.update()
			return
		}
		if err := p.live.system.Start(e.id, plain, paste, keep); err != nil {
			p.message = p.live.system.Message
		}
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
		p.message = "这条记录的内容已丢失。"
		p.update()
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
func (p *quickDemo) actions() []qpAction {
	if p.live != nil {
		available := p.active() != nil && !p.live.d.live.busy && (p.live.system == nil || !p.live.system.Busy)
		canPaste := available && p.live.system != nil && p.live.system.CanPaste
		return []qpAction{{"paste", "粘贴到原应用", "Enter", canPaste}, {"plain", "纯文本粘贴", "Shift+Enter", canPaste && p.active() != nil && (p.active().kind == "文本" || p.active().kind == "富文本")}, {"copy", "复制到剪贴板", p.commandLabel() + "C", available}, {"delete", "删除", "", available}, {"main", "打开主窗口", "", true}, {"settings", "设置…", "", true}}
	}
	e := p.active()
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
func (p *quickDemo) openActions() {
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
func (p *quickDemo) runAction(id string) {
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
func (p *quickDemo) navigate(down bool) {
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
	if len(p.rows) == 0 && len(p.suggestions()) == 0 {
		options := p.relaxations()
		if len(options) > 0 {
			step := 1
			if !down {
				step = -1
			}
			p.relaxCursor = (p.relaxCursor + step + len(options)) % len(options)
			p.update()
		}
		return
	}
	options := p.suggestions()
	if len(options) > 0 {
		if p.suggestFocused {
			if down && p.suggestCursor+1 < len(options) {
				p.suggestCursor++
			} else if down && len(p.rows) > 0 {
				p.suggestFocused = false
				p.suggestCursor = 0
			} else if !down {
				p.suggestCursor = max(0, p.suggestCursor-1)
			}
			p.update()
			return
		}
		columns := 1
		if p.kind == "图片" {
			columns = 3
		}
		if !down && p.selected < columns {
			p.suggestFocused = true
			p.suggestCursor = len(options) - 1
			p.update()
			return
		}
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
func (p *quickDemo) escape() {
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
	case p.query != "" || p.kind != "" || len(p.chips) > 0:
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

func (p *quickDemo) relaxations() []qpRelaxation {
	if p.live != nil {
		return p.liveRelaxations()
	}
	result := []qpRelaxation{}
	for _, dim := range []string{"type", "tag", "time", "source"} {
		values := []string{}
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
		label := ""
		switch dim {
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
		}
		// Copy only the fixture model used by matching. Never copy native callbacks.
		relaxed := quickDemo{kind: p.kind, query: p.query}
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
func (p *quickDemo) relax(dim string) {
	if dim == "type" {
		p.kind = ""
	}
	p.chips = slices.DeleteFunc(p.chips, func(f qpFilter) bool { return f.dimension == dim })
	p.queryChanged()
	p.suggestOpen = false
}

func (p *quickDemo) host(action string) {
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
