package desktopdemo

import (
	"fmt"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
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
	kind                    uikit.SearchTokenKind
}
type demoSearch struct {
	buffer, last, extension    string
	start, end, highlight      int
	open, initialized, restore bool
	options                    []searchCandidate
}

var searchDimensions = []string{"type", "tag", "source", "time", "extension"}
var searchPrefixes = map[string]string{"type": "/", "tag": "#", "source": "@", "time": "on:", "extension": "ext:"}
var searchLabels = map[string]string{"type": "类型", "tag": "标签", "source": "设备", "time": "时间", "extension": "扩展名"}
var searchKinds = map[string]uikit.SearchTokenKind{"type": uikit.SearchKindType, "tag": uikit.SearchKindTag, "source": uikit.SearchKindSource, "time": uikit.SearchKindTime, "extension": uikit.SearchKindOther}

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
func (d *demo) searchValues() []searchCandidate {
	var out []searchCandidate
	add := func(dim, value, label string) {
		out = append(out, searchCandidate{dim, value, label, searchKinds[dim]})
	}
	for _, v := range [][2]string{{"text", "文本"}, {"richtext", "富文本"}, {"image", "图片"}, {"file", "文件"}} {
		add("type", v[0], v[1])
	}
	for _, tag := range d.tags {
		add("tag", tag.id, tag.name)
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
func (d *demo) searchCandidates(dim, partial string) []searchCandidate {
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
func (s *demoSearch) active() (searchWord, bool) {
	for _, w := range searchWords(s.buffer) {
		if w.dimension != "" && w.start <= s.start && w.end >= s.end {
			return w, true
		}
	}
	return searchWord{}, false
}
func (s *demoSearch) replace(w searchWord, replacement string) {
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
func (s *demoSearch) refresh(d *demo) {
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
		s.options = d.searchCandidates("", s.buffer)
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
func (d *demo) setSearchCondition(dim, value string) {
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
func (s *demoSearch) apply(d *demo, index int) {
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
func (s *demoSearch) change(d *demo, start, end int) {
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
	s.open = true
	s.refresh(d)
	if w, ok := s.active(); ok && (w.partial == "" || len(s.options) > 0) {
		s.highlight = 0
	} else {
		s.highlight = -1
	}
}
func (s *demoSearch) clear(d *demo) {
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
func (s *demoSearch) removeBadge(d *demo, dimension string) {
	d.setSearchCondition(dimension, "")
	s.restore = true
	s.refresh(d)
}
func (s *demoSearch) tokens(d *demo) []uikit.SearchFieldToken {
	var out []uikit.SearchFieldToken
	add := func(dim, value string) {
		if value != "" && value != "all" {
			out = append(out, uikit.SearchFieldToken{ID: dim, Field: searchLabels[dim], Value: value, Kind: searchKinds[dim], RemoveLabel: "移除" + searchLabels[dim] + "条件"})
		}
	}
	add("type", d.kind)
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
				add("time", c.label)
				break
			}
		}
	}
	if s.extension != "" {
		add("extension", "."+s.extension)
	}
	return out
}
func (s *demoSearch) key(d *demo, key uikit.SearchFieldKey) bool {
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

func (d *demo) historySearch(c *ui.Context, k uikit.Kit) {
	s := &d.search
	s.refresh(d)
	restore := s.restore
	s.restore = false
	var suggestions []uikit.SearchSuggestion
	previous := ""
	for _, candidate := range s.options {
		trial := *d
		trial.setSearchCondition(candidate.dimension, candidate.value)
		trial.query = d.query
		n := len(trial.filtered())
		group := ""
		if candidate.dimension != previous {
			group = searchLabels[candidate.dimension]
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
		suggestions = append(suggestions, uikit.SearchSuggestion{ID: candidate.dimension + ":" + candidate.value, Label: candidate.label, Kind: candidate.kind, Group: group, Detail: fmt.Sprintf("%d 项", n), Selected: selected, Muted: n == 0})
	}
	feedback := ""
	if w, ok := s.active(); ok {
		if len(s.options) > 0 {
			feedback = "按 Enter 或 Tab 选择" + searchLabels[w.dimension] + "条件，保留已有搜索文字。"
		} else {
			feedback = "没有匹配的" + searchLabels[w.dimension] + "条件，按原文字搜索。"
		}
	}
	res := k.TokenSearchField(c, uikit.TokenSearchFieldProps{Toolbar: true, Label: "搜索历史", Value: &s.buffer, Placeholder: "搜索，或输入 # @ / on:", ShortcutHint: "⌘F", Tokens: s.tokens(d), Suggestions: suggestions, Open: &s.open, Highlighted: &s.highlight, Feedback: feedback, ClearLabel: "清除搜索", SuggestionsFooter: "↑↓ 选择 · Enter 或 Tab 转为筛选条件 · 输入框为空时退格删除最后一个条件", OnKey: func(key uikit.SearchFieldKey) bool { return s.key(d, key) }, OnSelectionChange: func(start, end int) {
		if !restore && !s.restore && (s.start != start || s.end != end) {
			s.start, s.end = start, end
			s.refresh(d)
			if _, ok := s.active(); ok {
				s.highlight = 0
			}
			c.Invalidate()
		}
	}})
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
		c.Invalidate()
	}
}

func (d *demo) matchesSearch(e entry) bool {
	if d.kind != "" && d.kind != "all" {
		if d.kind == "文本" {
			if e.kind != uikit.KindText && e.kind != uikit.KindCode && e.kind != uikit.KindLink {
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
		if e.kind == uikit.KindImage || e.kind == uikit.KindFile {
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
