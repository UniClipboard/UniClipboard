package desktopdemo

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"time"

	"github.com/uniclipboard/unic/engine"
)

func searchQueryKey(q engine.HistorySearchQuery) string { b, _ := json.Marshal(q); return string(b) }
func (d *demo) historySearchQuery() engine.HistorySearchQuery {
	if d.live != nil && d.live.queryOverride != nil {
		return d.live.queryOverride()
	}
	base := d.liveQuery()
	q := engine.HistorySearchQuery{Tags: selectedSearchTags(d.tag), TagMatch: "all", CreatedFromMs: base.FromMs, CreatedBeforeMs: base.BeforeMs}
	for _, w := range searchWords(d.query) {
		q.TextTerms = append(q.TextTerms, literalSearchWord(w.raw))
	}
	switch d.kind {
	case "文本", "text":
		q.Types = []engine.HistorySearchType{"text"}
	case "富文本", "richtext":
		q.Types = []engine.HistorySearchType{"richtext"}
	case "图片", "image":
		q.Types = []engine.HistorySearchType{"image"}
	case "文件", "file":
		q.Types = []engine.HistorySearchType{"files"}
	}
	if d.source != "" && d.source != "all" {
		q.Sources = []engine.HistorySearchSource{searchSource(d.source)}
	}
	if d.search.extension != "" {
		q.Extensions = []string{d.search.extension}
	}
	return q
}
func searchSource(value string) engine.HistorySearchSource {
	switch value {
	case "local", "external", "migration":
		return engine.HistorySearchSource{Kind: value}
	default:
		return engine.HistorySearchSource{Kind: "device", DeviceID: engine.DeviceID(value)}
	}
}
func (l *liveState) loadCatalogue(d *demo) {
	prefix := d.search.buffer
	field := ""
	w, active := d.search.active()
	if !active {
		for _, word := range searchWords(d.search.buffer) {
			if word.dimension != "" && word.committed {
				w = word
				active = true
			}
		}
	}
	if active {
		prefix = w.partial
		field = map[string]string{"tag": "tag", "source": "source", "extension": "extension"}[w.dimension]
		if field == "" {
			prefix = ""
		}
	}
	keyBytes, _ := json.Marshal([]any{field, prefix, l.appliedSerial})
	key := string(keyBytes)
	if key == l.catalogueKey {
		return
	}
	l.catalogueKey = key
	if l.catalogueCancel != nil {
		l.catalogueCancel()
	}
	ctx, cancel := context.WithTimeout(l.ctx, 6*time.Second)
	l.catalogueCancel = cancel
	client := l.client
	go func() {
		defer cancel()
		values := map[string][]engine.HistorySearchCandidate{}
		for _, f := range []string{"tag", "source", "extension"} {
			p := ""
			if field == "" || field == f {
				p = prefix
			}
			page, e := client.ListHistorySearchCandidates(ctx, engine.HistorySearchCandidatesRequest{Field: f, Prefix: p, Limit: 200})
			if e != nil {
				return
			}
			values[f] = page.Candidates
		}
		select {
		case l.results <- func() {
			if key != l.catalogueKey {
				return
			}
			l.catalogue = values
			if l.queryOverride == nil && d.search.initialized {
				d.search.change(d, d.search.start, d.search.end)
			}
		}:
		case <-l.ctx.Done():
		}
	}()
}
func (l *liveState) requestCounts(queries []engine.HistorySearchQuery) {
	if len(queries) > 32 {
		queries = queries[:32]
	}
	b, _ := json.Marshal(struct {
		Queries  []engine.HistorySearchQuery
		Revision uint64
	}{queries, l.appliedSerial})
	key := string(b)
	if key == l.countKey {
		return
	}
	l.countKey = key
	if l.countCancel != nil {
		l.countCancel()
	}
	ctx, cancel := context.WithTimeout(l.ctx, 4*time.Second)
	l.countCancel = cancel
	client := l.client
	go func() {
		defer cancel()
		out, e := client.CountHistorySearches(ctx, engine.HistorySearchCountsRequest{Queries: queries})
		select {
		case l.results <- func() {
			if key != l.countKey || e != nil {
				return
			}
			l.counts = map[string]uint64{}
			for i, n := range out.Counts {
				l.counts[searchQueryKey(queries[i])] = n
			}
		}:
		case <-l.ctx.Done():
		}
	}()
}
func (d *demo) liveSearchValues() []searchCandidate {
	out := []searchCandidate{}
	add := func(dim, value, label string) {
		out = append(out, searchCandidate{dimension: dim, value: value, label: label, kind: searchKinds[dim]})
	}
	for _, v := range [][2]string{{"text", "文本"}, {"richtext", "富文本"}, {"image", "图片"}, {"file", "文件"}} {
		add("type", v[0], v[1])
	}
	for _, f := range []struct{ field, dim string }{{"tag", "tag"}, {"source", "source"}, {"extension", "extension"}} {
		for _, v := range d.live.catalogue[f.field] {
			label := v.Label
			if f.field == "extension" {
				label = "." + v.Value
			}
			add(f.dim, v.Value, label)
		}
	}
	for _, tag := range d.tags {
		if !slices.ContainsFunc(out, func(v searchCandidate) bool { return v.dimension == "tag" && v.value == tag.id }) {
			add("tag", tag.id, tag.name)
		}
	}
	for _, v := range [][2]string{{"today", "今天"}, {"yesterday", "昨天"}, {"last_7d", "最近 7 天"}, {"last_30d", "最近 30 天"}, {"this_week", "本周"}, {"this_month", "本月"}} {
		add("time", v[0], v[1])
	}
	return out
}
func (l *liveState) invalidateSearch() {
	l.loadGeneration++
	if l.searchCancel != nil {
		l.searchCancel()
	}
	l.loading = false
	l.countKey = ""
	l.counts = nil
	if l.countCancel != nil {
		l.countCancel()
	}
}
func copySearchQuery(q engine.HistorySearchQuery) engine.HistorySearchQuery {
	q.TextTerms = slices.Clone(q.TextTerms)
	q.Tags = slices.Clone(q.Tags)
	q.Types = slices.Clone(q.Types)
	q.Sources = slices.Clone(q.Sources)
	q.Extensions = slices.Clone(q.Extensions)
	return q
}
func queryLiteral(s string) []string {
	if strings.TrimSpace(s) == "" {
		return nil
	}
	return []string{strings.TrimSpace(s)}
}
