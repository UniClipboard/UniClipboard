package desktopui

import (
	"fmt"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/desktopkeys"
	"github.com/uniclipboard/unic/plugin"
)

// The model follows main's composite-search model and token-range controller.
// Offsets are runes, matching mygo's editor rather than JavaScript's UTF-16.
type searchWord struct {
	start, end              int
	dimension, partial, raw string
	committed               bool
}
type searchCandidate struct {
	dimension, value, label string
	kind                    desktopkit.SearchTokenKind
}
type historySearchState struct {
	buffer, last, extension              string
	start, end, highlight                int
	open, initialized, restore           bool
	wasFocused, hadActivity, returnFocus bool
	options                              []searchCandidate
}

var searchDimensions = []string{"type", "tag", "source", "time", "extension"}
var searchPrefixes = map[string]string{"type": "/", "tag": "#", "source": "@", "time": "on:", "extension": "ext:"}
var searchLabels = map[string]string{"type": "类型", "tag": "标签", "source": "设备", "time": "时间", "extension": "扩展名"}
var searchKinds = map[string]desktopkit.SearchTokenKind{"type": desktopkit.SearchKindType, "tag": desktopkit.SearchKindTag, "source": desktopkit.SearchKindSource, "time": desktopkit.SearchKindTime, "extension": desktopkit.SearchKindOther}

func searchWords(buffer string) []searchWord {
	r := []rune(buffer)
	var out []searchWord
	for i := 0; i < len(r); {
		if unicode.IsSpace(r[i]) {
			i++
			continue
		}
		start := i
		var quote rune
		for i < len(r) {
			ch := r[i]
			if ch == '\\' && i+1 < len(r) {
				i += 2
				continue
			}
			if quote != 0 {
				if ch == quote {
					quote = 0
				}
				i++
				continue
			}
			if ch == '"' || ch == '\'' {
				quote = ch
				i++
				continue
			}
			if unicode.IsSpace(ch) {
				break
			}
			i++
		}
		raw := string(r[start:i])
		w := searchWord{start: start, end: i, raw: raw, committed: i < len(r) && unicode.IsSpace(r[i])}
		if r[start] != '"' && r[start] != '\'' && r[start] != '\\' {
			for _, dim := range searchDimensions {
				prefix := searchPrefixes[dim]
				if strings.HasPrefix(strings.ToLower(raw), prefix) {
					w.dimension = dim
					w.partial = literalSearchWord(raw[len(prefix):])
					break
				}
			}
			if w.dimension == "type" && strings.ContainsAny(w.partial, "/\\") {
				w.dimension = ""
			}
		}
		out = append(out, w)
	}
	return out
}

func literalSearchWord(s string) string {
	if strings.HasPrefix(s, "\"") {
		if v, err := strconv.Unquote(s); err == nil {
			return v
		}
	}
	if len(s) >= 2 && s[0] == '\'' && s[len(s)-1] == '\'' {
		s = s[1 : len(s)-1]
	}
	var out strings.Builder
	escape := false
	for _, r := range s {
		if escape {
			out.WriteRune(r)
			escape = false
		} else if r == '\\' {
			escape = true
		} else {
			out.WriteRune(r)
		}
	}
	if escape {
		out.WriteRune('\\')
	}
	return out.String()
}

func selectedSearchTags(value string) []string {
	if value == "" || value == "all" {
		return nil
	}
	return strings.Split(value, "\x1f")
}
func (d *viewState) searchValues() []searchCandidate {
	if d.live != nil {
		return d.liveSearchValues()
	}
	var out []searchCandidate
	add := func(dim, value, label string) {
		out = append(out, searchCandidate{dim, value, label, searchKinds[dim]})
	}
	for _, v := range [][2]string{{"text", "文本"}, {"richtext", "富文本"}, {"image", "图片"}, {"file", "文件"}, {"folder", "文件夹"}} {
		add("type", v[0], v[1])
	}
	for _, tag := range d.tags {
		add("tag", tag.id, d.tagLabel(tag.id))
	}
	for _, v := range d.devices {
		add("source", v.id, d.sourceName(v.id))
	}
	for _, v := range [][2]string{{"today", "今天"}, {"yesterday", "昨天"}, {"last_7d", "最近 7 天"}, {"last_30d", "最近 30 天"}, {"this_week", "本周"}, {"this_month", "本月"}} {
		add("time", v[0], v[1])
	}
	for _, ext := range []string{"txt", "md", "jpg", "png", "pdf", "ts", "js", "json", "rs", "go"} {
		add("extension", ext, "."+ext)
	}
	return out
}
func (d *viewState) searchCandidates(dim, partial string) []searchCandidate {
	if d.live != nil && dim == "tag" && partial == plugin.TagFavorite {
		return nil
	}
	needle := strings.ToLower(partial)
	var out []searchCandidate
	rank := func(c searchCandidate) int {
		for _, v := range []string{c.value, c.label} {
			if strings.EqualFold(v, partial) {
				return 0
			}
		}
		for _, v := range []string{c.value, c.label} {
			if strings.HasPrefix(strings.ToLower(v), needle) {
				return 1
			}
		}
		return 2
	}
	for _, c := range d.searchValues() {
		if (dim == "" || dim == c.dimension) && (needle == "" || strings.Contains(strings.ToLower(c.value), needle) || strings.Contains(strings.ToLower(c.label), needle)) {
			out = append(out, c)
		}
	}
	if dim != "" {
		slices.SortStableFunc(out, func(a, b searchCandidate) int { return rank(a) - rank(b) })
	}
	return out
}
func (s *historySearchState) active() (searchWord, bool) {
	for _, w := range searchWords(s.buffer) {
		if w.dimension != "" && w.start <= s.start && w.end >= s.end {
			return w, true
		}
	}
	return searchWord{}, false
}
func (s *historySearchState) replace(w searchWord, replacement string) {
	r := []rune(s.buffer)
	s.buffer = string(r[:w.start]) + replacement + string(r[w.end:])
	s.start = w.start + len([]rune(replacement))
	s.end = s.start
	if strings.TrimSpace(s.buffer) == "" {
		s.buffer = ""
		s.start, s.end = 0, 0
	}
	s.restore = true
	s.last = s.buffer
}
func (s *historySearchState) refresh(d *viewState) {
	if !s.initialized {
		s.buffer = d.query
		s.last = s.buffer
		s.start = len([]rune(s.buffer))
		s.end = s.start
		s.highlight = -1
		s.initialized = true
	}
	w, typed := s.active()
	if typed {
		s.options = d.searchCandidates(w.dimension, w.partial)
	} else {
		s.options = nil
	}
	d.query = s.buffer
	if s.open && typed && (w.partial == "" || len(s.options) > 0) {
		r := []rune(s.buffer)
		d.query = strings.TrimSpace(string(r[:w.start]) + string(r[w.end:]))
	}
	if s.highlight >= len(s.options) {
		s.highlight = len(s.options) - 1
	}
}
func (d *viewState) setSearchCondition(dim, value string) {
	switch dim {
	case "type":
		d.kind = value
		for _, v := range d.searchValues() {
			if v.dimension == dim && v.value == value {
				d.kind = v.label
				break
			}
		}
	case "tag":
		tags := selectedSearchTags(d.tag)
		if value == "" {
			tags = nil
		} else if i := slices.Index(tags, value); i >= 0 {
			tags = slices.Delete(tags, i, i+1)
		} else {
			tags = append(tags, value)
		}
		d.tag = strings.Join(tags, "\x1f")
	case "source":
		d.source = value
	case "time":
		d.timeRange = value
	case "extension":
		d.search.extension = value
	}
	if (dim == "tag" && strings.HasPrefix(d.nav.Value, "tag:")) || (dim == "source" && strings.HasPrefix(d.nav.Value, "device:")) {
		d.nav.Value = "all"
	}
}
func (s *historySearchState) apply(d *viewState, index int) {
	if index < 0 || index >= len(s.options) {
		return
	}
	c := s.options[index]
	d.setSearchCondition(c.dimension, c.value)
	if w, ok := s.active(); ok {
		s.replace(w, "")
	}
	s.open = false
	s.highlight = -1
	s.last = s.buffer
	s.restore = true
	s.refresh(d)
}
func (s *historySearchState) change(d *viewState, start, end int) {
	s.start, s.end = start, end
	// Completed exact words commit left-to-right, including pasted conditions.
	var removed []searchWord
	for _, w := range searchWords(s.buffer) {
		if w.dimension == "" || !w.committed || w.end >= s.start || w.partial == "" {
			continue
		}
		for _, c := range d.searchCandidates(w.dimension, w.partial) {
			if strings.EqualFold(c.value, w.partial) || strings.EqualFold(c.label, w.partial) {
				if c.dimension != "tag" || !slices.Contains(selectedSearchTags(d.tag), c.value) {
					d.setSearchCondition(c.dimension, c.value)
				}
				removed = append(removed, w)
				break
			}
		}
	}
	for i := len(removed) - 1; i >= 0; i-- {
		w := removed[i]
		a, b := s.start, s.end
		s.replace(w, "")
		s.start = a - (w.end - w.start)
		s.end = b - (w.end - w.start)
	}
	if strings.TrimSpace(s.buffer) == "" {
		s.buffer = ""
		s.start, s.end = 0, 0
	}
	s.last = s.buffer
	s.refresh(d)
	if w, ok := s.active(); ok && (w.partial == "" || len(s.options) > 0) {
		s.open = true
		s.highlight = 0
	} else {
		s.open = false
		s.highlight = -1
	}
}
func (s *historySearchState) clear(d *viewState) {
	for _, dim := range searchDimensions {
		d.setSearchCondition(dim, "")
	}
	s.buffer, s.last = "", ""
	s.start, s.end = 0, 0
	s.highlight = -1
	s.open = false
	s.restore = true
	s.refresh(d)
}

// A badge is one filter dimension, including every tag in a multi-tag badge.
func (s *historySearchState) removeBadge(d *viewState, dimension string) {
	d.setSearchCondition(dimension, "")
	s.restore = true
	s.refresh(d)
}
func (s *historySearchState) tokens(d *viewState) []desktopkit.SearchFieldToken {
	var out []desktopkit.SearchFieldToken
	add := func(dim, value string) {
		if value != "" && value != "all" {
			out = append(out, desktopkit.SearchFieldToken{ID: dim, Field: d.tr(searchLabels[dim]), Value: value, Kind: searchKinds[dim], RemoveLabel: fmt.Sprintf(d.tr("移除%s条件"), d.tr(searchLabels[dim]))})
		}
	}
	add("type", d.tr(d.kind))
	tags := selectedSearchTags(d.tag)
	if len(tags) > 0 {
		labels := make([]string, len(tags))
		for i, id := range tags {
			labels[i] = d.tagLabel(id)
		}
		add("tag", "#"+strings.Join(labels, ", #"))
	}
	if d.source != "" && d.source != "all" {
		add("source", d.sourceName(d.source))
	}
	if d.timeRange != "" && d.timeRange != "all" {
		for _, c := range d.searchValues() {
			if c.dimension == "time" && c.value == d.timeRange {
				add("time", d.tr(c.label))
				break
			}
		}
	}
	if s.extension != "" {
		add("extension", "."+s.extension)
	}
	return out
}
func (s *historySearchState) key(d *viewState, key desktopkit.SearchFieldKey) bool {
	s.buffer = key.Text
	s.start, s.end = key.Start, key.End
	s.refresh(d)
	switch key.Key {
	case ui.KeyEscape:
		if s.open {
			s.open = false
			s.highlight = -1
			d.query = s.buffer
			return true
		}
		if s.buffer != "" || len(s.tokens(d)) > 0 {
			s.clear(d)
			return true
		}
		return false
	case ui.KeyBackspace:
		chips := s.tokens(d)
		if s.buffer != "" || len(chips) == 0 {
			return false
		}
		s.removeBadge(d, chips[len(chips)-1].ID)
		return true
	case ui.KeyDown, ui.KeyUp:
		if len(s.options) == 0 {
			return false
		}
		s.open = true
		if s.highlight < 0 && key.Key == ui.KeyUp {
			s.highlight = len(s.options) - 1
		} else if key.Key == ui.KeyDown {
			s.highlight = (s.highlight + 1) % len(s.options)
		} else {
			s.highlight = (s.highlight - 1 + len(s.options)) % len(s.options)
		}
		s.refresh(d)
		return true
	case ui.KeyTab:
		if key.Shift() {
			return false
		}
		if _, ok := s.active(); !ok || len(s.options) == 0 {
			return false
		}
		s.apply(d, max(s.highlight, 0))
		return true
	case ui.KeyEnter:
		if s.open && s.highlight >= 0 {
			s.apply(d, s.highlight)
		} else {
			s.open = false
			s.refresh(d)
		}
		return true
	}
	return false
}

func (d *viewState) historySearch(c *ui.Context, k desktopkit.Kit, tm desktopkit.ToolbarMetrics) {
	s := &d.search
	s.refresh(d)
	restore := s.restore
	s.restore = false
	collapsed := tm.Narrow && !d.narrowSearchOpen
	takeover := tm.Narrow && d.narrowSearchOpen
	returnFocus := false
	if collapsed && s.returnFocus {
		returnFocus = true
		s.returnFocus = false
	}
	var suggestions []desktopkit.SearchSuggestion
	var prospective []engine.HistorySearchQuery
	previous := ""
	for _, candidate := range s.options {
		trial := *d
		trial.setSearchCondition(candidate.dimension, candidate.value)
		trial.query = d.query
		n := len(trial.filtered())
		group := ""
		if candidate.dimension != previous {
			group = d.tr(searchLabels[candidate.dimension])
			previous = candidate.dimension
		}
		selected := false
		switch candidate.dimension {
		case "type":
			selected = d.kind == candidate.label
		case "source":
			selected = d.source == candidate.value
		case "tag":
			selected = slices.Contains(selectedSearchTags(d.tag), candidate.value)
		case "time":
			selected = d.timeRange == candidate.value
		case "extension":
			selected = s.extension == candidate.value
		}
		detail := fmt.Sprintf(d.tr("%d 项"), n)
		if d.live != nil {
			detail = ""
			q := trial.historySearchQuery()
			prospective = append(prospective, q)
			if count, ok := d.live.counts[searchQueryKey(q)]; ok {
				detail = fmt.Sprintf("%d 项", count)
				n = int(count)
			}
		}
		suggestions = append(suggestions, desktopkit.SearchSuggestion{ID: candidate.dimension + ":" + candidate.value, Label: d.searchCandidateLabel(candidate), Kind: candidate.kind, Group: group, Detail: detail, Selected: selected, Muted: n == 0 && (d.live == nil || detail != "")})
	}
	if d.live != nil {
		if d.filterOpen && len(prospective) == 0 {
			prospective = d.filterPopoverProspectiveQueries()
		}
		if len(d.entries) == 0 && !d.live.foregroundLoading && d.hasActiveSearchOrFilter() && !d.unsupportedSearch() && d.live.readError == "" {
			prospective = append(prospective, d.searchRelaxationQueries()...)
		}
		if len(prospective) > 0 {
			d.live.requestCounts(prospective)
		}
	}
	feedback := ""
	if w, ok := s.active(); ok {
		if len(s.options) > 0 {
			feedback = d.tr("按 Enter 或 Tab 选择") + searchLabels[w.dimension] + d.tr("条件，保留已有搜索文字。")
		} else {
			feedback = d.tr("没有匹配的") + searchLabels[w.dimension] + d.tr("条件，按原文字搜索。")
		}
	}
	filterCount := d.activeFilterCount()
	var overrides map[string][]string
	if d.live != nil {
		overrides = d.live.config.Config.Desktop.Shortcuts
	}
	activeTooltip := ""
	if collapsed && (strings.TrimSpace(s.buffer) != "" || filterCount > 0) {
		var parts []string
		if q := strings.TrimSpace(s.buffer); q != "" {
			parts = append(parts, q)
		}
		if filterCount > 0 {
			parts = append(parts, fmt.Sprintf("%s (%d)", d.tr("筛选条件"), filterCount))
		}
		activeTooltip = d.tr("搜索历史") + " · " + strings.Join(parts, " · ")
	}
	res := k.TokenSearchField(c, desktopkit.TokenSearchFieldProps{ExternalShortcut: true,
		Toolbar:        true,
		Unified:        true,
		Compact:        tm.Compact,
		Collapsed:      collapsed,
		NarrowTakeover: takeover,
		ActiveTooltip:  activeTooltip,
		ReturnFocus:    returnFocus,
		OnExpand: func() {
			d.narrowSearchOpen = true
			s.restore = true
			s.hadActivity = false
			c.Invalidate()
		},
		OnCollapse: func() {
			d.narrowSearchOpen = false
			d.filterOpen = false
			s.open = false
			s.returnFocus = true
			c.Invalidate()
		},
		Label:             d.tr("搜索历史"),
		Value:             &s.buffer,
		Placeholder:       d.tr("搜索或筛选…"),
		ShortcutHint:      shortcutHint(overrides, desktopkeys.Search),
		Tokens:            s.tokens(d),
		Suggestions:       suggestions,
		Open:              &s.open,
		Highlighted:       &s.highlight,
		Feedback:          feedback,
		ClearLabel:        d.tr("清除搜索"),
		SuggestionsFooter: d.tr("↑↓ 选择 · Enter 或 Tab 转为筛选条件 · 输入框为空时退格删除最后一个条件"),
		FilterOpen:        &d.filterOpen,
		FilterCount:       filterCount,
		FilterLabel:       d.tr("筛选条件"),
		FilterPanel:       func() { d.filterPopover(c.Key("filter-popover"), k) },
		Loading:           d.live != nil && d.live.showForegroundLoading(),
		OnKey:             func(key desktopkit.SearchFieldKey) bool { return s.key(d, key) },
		OnSelectionChange: func(start, end int) {
			if !restore && !s.restore && (s.start != start || s.end != end) {
				s.start, s.end = start, end
				s.refresh(d)
				if _, ok := s.active(); ok {
					s.open = true
					s.highlight = 0
				} else {
					s.open = false
				}
				c.Invalidate()
			}
		},
	})
	if collapsed {
		s.wasFocused = false
		s.hadActivity = false
		return
	}
	if restore {
		res.Input.SetTextSelection(s.start, s.end).Focus()
	}
	if res.Edited && s.buffer != s.last {
		a, b := res.Input.TextSelection()
		s.change(d, a, b)
		c.Invalidate()
	}
	if res.Selected >= 0 {
		s.apply(d, res.Selected)
		c.Invalidate()
	}
	if res.RemovedToken != "" {
		s.removeBadge(d, res.RemovedToken)
		c.Invalidate()
	}
	if res.Cleared {
		s.clear(d)
		d.filterOpen = false
		c.Invalidate()
	}
	s.wasFocused = res.Focused
	if takeover {
		activeFocus := res.Focused || d.filterOpen
		if s.hadActivity && !activeFocus && !restore && !s.restore {
			d.narrowSearchOpen = false
			c.Invalidate()
		}
		if activeFocus {
			s.hadActivity = true
		}
	} else {
		s.hadActivity = false
	}
}

func (d *viewState) activeFilterCount() int {
	n := 0
	if d.kind != "" && d.kind != "all" {
		n++
	}
	if d.source != "" && d.source != "all" {
		n++
	}
	n += len(selectedSearchTags(d.tag))
	if d.timeRange != "" && d.timeRange != "all" {
		n++
	}
	if d.search.extension != "" {
		n++
	}
	return n
}

func (d *viewState) filterPopoverKinds() []string {
	if d.live != nil {
		return []string{"文本", "富文本", "图片", "文件", "文件夹"}
	}
	return []string{"文本", "代码", "链接", "图片", "文件", "文件夹"}
}

func (d *viewState) filterPopoverSources() [][2]string {
	out := make([][2]string, 0, len(d.devices))
	seen := make(map[string]bool, len(d.devices))
	for _, dev := range d.devices {
		out = append(out, [2]string{dev.id, d.sourceName(dev.id)})
		seen[dev.id] = true
	}
	if d.live != nil {
		for _, cand := range d.live.catalogue["source"] {
			if cand.Value == "" || seen[cand.Value] {
				continue
			}
			label := cand.Label
			if label == "" {
				label = d.sourceName(cand.Value)
			}
			out = append(out, [2]string{cand.Value, label})
			seen[cand.Value] = true
		}
	}
	return out
}

func (d *viewState) filterPopoverTags() []historyTag {
	var custom []historyTag
	for _, tag := range d.tags {
		if !tag.builtin {
			custom = append(custom, tag)
		}
	}
	return custom
}

func filterPopoverTimes() [][2]string {
	return [][2]string{{"today", "今天"}, {"yesterday", "昨天"}, {"last_7d", "最近 7 天"}, {"last_30d", "最近 30 天"}}
}

func (d *viewState) filterPopoverTrialQuery(dim, val string) engine.HistorySearchQuery {
	trial := *d
	switch dim {
	case "type":
		trial.kind = val
	case "source":
		trial.source = val
	case "tag":
		tags := selectedSearchTags(d.tag)
		if !slices.Contains(tags, val) {
			tags = append(slices.Clone(tags), val)
		}
		trial.tag = strings.Join(tags, "\x1f")
	case "time":
		trial.timeRange = val
	}
	return trial.historySearchQuery()
}

func (d *viewState) filterPopoverProspectiveQueries() []engine.HistorySearchQuery {
	var queries []engine.HistorySearchQuery
	for _, k := range d.filterPopoverKinds() {
		queries = append(queries, d.filterPopoverTrialQuery("type", k))
	}
	for _, src := range d.filterPopoverSources() {
		queries = append(queries, d.filterPopoverTrialQuery("source", src[0]))
	}
	for _, tag := range d.filterPopoverTags() {
		queries = append(queries, d.filterPopoverTrialQuery("tag", tag.id))
	}
	for _, tm := range filterPopoverTimes() {
		queries = append(queries, d.filterPopoverTrialQuery("time", tm[0]))
	}
	return queries
}

// filterPopover renders the U1 integrated chip-based filter popover anchored to the
// unified omnibar's trailing filter button.
func (d *viewState) filterPopover(c *ui.Context, k desktopkit.Kit) {
	countFor := func(dim, val string) (int, bool) {
		if d.live != nil {
			q := d.filterPopoverTrialQuery(dim, val)
			if cnt, ok := d.live.counts[searchQueryKey(q)]; ok {
				return int(cnt), true
			}
			return 0, false
		}
		trial := *d
		switch dim {
		case "type":
			trial.kind = val
		case "source":
			trial.source = val
		case "tag":
			tags := selectedSearchTags(d.tag)
			if !slices.Contains(tags, val) {
				tags = append(slices.Clone(tags), val)
			}
			trial.tag = strings.Join(tags, "\x1f")
		case "time":
			trial.timeRange = val
		}
		return len(trial.filtered()), true
	}

	renderChip := func(key, label string, active bool, cnt int, hasCnt bool, swatch *desktopkit.TagColor, onClick func()) {
		btn := ui.ButtonBase(c.Key(key)).Height(26).PaddingX(9).Radius(7).Gap(5).AlignItems(ui.Center).
			Label(label).KeepFocus()
		fg := k.Color("text-primary")
		cntFg := k.Color("text-tertiary")
		if active {
			bgAlpha := float32(.11)
			borderAlpha := float32(.48)
			if k.Dark {
				bgAlpha = .20
				borderAlpha = .62
			}
			btn.Background(k.Color("accent-500").Alpha(bgAlpha)).Border(1, k.Color("accent-500").Alpha(borderAlpha))
			fg = k.Color("accent-500")
			cntFg = k.Color("accent-500").Alpha(.78)
		} else if btn.Hovered() {
			btn.Background(k.Color("background-tertiary-default")).Border(1, k.Color("border-button-hover"))
		} else {
			btn.Background(k.SubtleCardSurface()).Border(1, k.Color("separator-border"))
		}
		btn.Children(func() {
			if swatch != nil {
				ui.Box(c.Key("dot")).Size(6, 6).Radius(3).Background(k.SwatchColor(*swatch)).Shrink(0).AccessHidden(true)
			}
			w := 500
			if active {
				w = 600
			}
			ui.Text(c.Key("l"), label).FontSize(12).FontWeight(w).TextColor(fg).SingleLine()
			if hasCnt {
				ui.Text(c.Key("c"), fmt.Sprint(cnt)).FontSize(10.5).TextColor(cntFg).SingleLine().AccessHidden(true)
			}
		})
		if btn.Clicked() {
			onClick()
			c.Invalidate()
		}
	}

	section := func(key, title string, body func()) {
		ui.Column(c.Key(key)).Gap(6).Children(func() {
			ui.Text(c.Key("h"), title).FontSize(11).FontWeight(600).TextColor(k.Color("text-tertiary")).SingleLine()
			ui.Row(c.Key("chips")).Gap(6).Wrap().Children(body)
		})
	}

	ui.Column(c).Padding(12, 14).Gap(12).Children(func() {
		ui.Row(c.Key("head")).AlignItems(ui.Center).Gap(8).Children(func() {
			ui.Text(c.Key("t"), d.tr("筛选条件")).FontSize(12.5).FontWeight(600).TextColor(k.Color("text-primary")).Grow(1)
			if d.activeFilterCount() > 0 {
				clr := ui.ButtonBase(c.Key("clear-all")).Height(22).PaddingX(7).Radius(5).Center().
					Label(d.tr("清除全部")).KeepFocus()
				if clr.Hovered() {
					clr.Background(k.Color("background-secondary-hover"))
				}
				clr.Children(func() {
					ui.Text(c, d.tr("清除全部")).FontSize(11.5).FontWeight(500).TextColor(k.Color("accent-500")).SingleLine()
				})
				if clr.Clicked() {
					d.search.clear(d)
					d.filterOpen = false
					c.Invalidate()
				}
			}
		})

		section("type", d.tr("内容类型"), func() {
			for _, v := range d.filterPopoverKinds() {
				val := v
				active := d.kind == val
				cnt, ok := countFor("type", val)
				renderChip(val, d.tr(val), active, cnt, ok, nil, func() {
					if d.kind == val {
						d.kind = ""
					} else {
						d.kind = val
					}
					d.search.refresh(d)
				})
			}
		})

		if sources := d.filterPopoverSources(); len(sources) > 0 {
			section("source", d.tr("来源设备"), func() {
				for _, src := range sources {
					id, label := src[0], src[1]
					active := d.source == id
					cnt, ok := countFor("source", id)
					renderChip(id, label, active, cnt, ok, nil, func() {
						if d.source == id {
							d.source = ""
						} else {
							d.source = id
						}
						if strings.HasPrefix(d.nav.Value, "device:") {
							d.nav.Value = "all"
						}
						d.search.refresh(d)
					})
				}
			})
		}

		if customTags := d.filterPopoverTags(); len(customTags) > 0 {
			section("tag", d.tr("标签"), func() {
				selectedTags := selectedSearchTags(d.tag)
				for _, tag := range customTags {
					id := tag.id
					col := tag.color
					active := slices.Contains(selectedTags, id)
					cnt, ok := countFor("tag", id)
					renderChip(id, d.tagLabel(id), active, cnt, ok, &col, func() {
						d.setSearchCondition("tag", id)
						d.search.refresh(d)
					})
				}
			})
		}

		section("time", d.tr("时间范围"), func() {
			for _, item := range filterPopoverTimes() {
				val, lbl := item[0], item[1]
				active := d.timeRange == val
				cnt, ok := countFor("time", val)
				renderChip(val, d.tr(lbl), active, cnt, ok, nil, func() {
					if d.timeRange == val {
						d.timeRange = ""
					} else {
						d.timeRange = val
					}
					d.search.refresh(d)
				})
			}
		})
	})
}

func (d *viewState) matchesSearch(e entry) bool {
	if d.kind != "" && d.kind != "all" {
		if d.kind == "文本" {
			if e.kind != desktopkit.KindText && e.kind != desktopkit.KindCode && e.kind != desktopkit.KindLink {
				return false
			}
		} else if d.kind != kindName(e.kind) {
			return false
		}
	}
	if d.source != "" && d.source != "all" && d.source != e.source {
		return false
	}
	for _, tag := range selectedSearchTags(d.tag) {
		if !d.entryHasTag(e, tag) {
			return false
		}
	}
	switch d.timeRange {
	case "today", "this_week":
		if !strings.HasPrefix(e.date, "今天") {
			return false
		}
	case "yesterday":
		if !strings.HasPrefix(e.date, "昨天") {
			return false
		}
	}
	if d.search.extension != "" {
		found := false
		if e.kind == desktopkit.KindImage || e.kind == desktopkit.KindFile {
			for _, name := range append([]string{e.title}, strings.Split(e.text, "\n")...) {
				if strings.EqualFold(strings.TrimPrefix(filepath.Ext(name), "."), d.search.extension) {
					found = true
					break
				}
			}
		}
		if !found {
			return false
		}
	}
	haystack := strings.ToLower(e.title + "\n" + e.text)
	for _, word := range searchWords(d.query) {
		if !strings.Contains(haystack, strings.ToLower(literalSearchWord(word.raw))) {
			return false
		}
	}
	return true
}
