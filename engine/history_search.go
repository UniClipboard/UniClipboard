package engine

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"time"
	"unicode/utf8"

	"golang.org/x/net/html"

	"github.com/uniclipboard/unic/internal/store"
)

func normalizeSearch(q HistorySearchQuery) (HistorySearchQuery, *Error) {
	bad := func() (HistorySearchQuery, *Error) {
		return q, newErr(CodeInvalidArgument, "invalid history search query")
	}
	if len(q.TextTerms) > 16 || len(q.Types) > 64 || len(q.Tags) > 64 || len(q.Sources) > 64 || len(q.Extensions) > 64 {
		return bad()
	}
	q.TextTerms = slices.Clone(q.TextTerms)
	total := 0
	for i, t := range q.TextTerms {
		if !utf8.ValidString(t) || len(t) > 4096 || strings.ContainsRune(t, 0) {
			return bad()
		}
		total += len(t)
		q.TextTerms[i] = strings.ToLower(t)
	}
	if total > 8192 {
		return bad()
	}
	q.TextTerms = slices.DeleteFunc(q.TextTerms, func(t string) bool { return t == "" })
	q.TextTerms = slices.Compact(slices.Sorted(slices.Values(q.TextTerms)))
	q.Types = slices.Compact(slices.Sorted(slices.Values(q.Types)))
	for _, t := range q.Types {
		if t != "text" && t != "richtext" && t != "image" && t != "files" && t != "directory" && t != "directory_reference" {
			return bad()
		}
	}
	tags, e := normalizeTags(q.Tags)
	if e != nil {
		return bad()
	}
	q.Tags = tags
	if q.TagMatch == "" {
		q.TagMatch = "all"
	}
	if q.TagMatch != "all" && q.TagMatch != "any" {
		return bad()
	}
	if len(q.Tags) == 0 {
		q.TagMatch = "all"
	}
	q.Extensions = slices.Clone(q.Extensions)
	for i, v := range q.Extensions {
		v = strings.ToLower(strings.TrimPrefix(v, "."))
		if v == "" || len(v) > 64 || !utf8.ValidString(v) || strings.ContainsAny(v, "/\\\x00") {
			return bad()
		}
		q.Extensions[i] = v
	}
	q.Extensions = slices.Compact(slices.Sorted(slices.Values(q.Extensions)))
	q.Sources = slices.Clone(q.Sources)
	for _, s := range q.Sources {
		switch s.Kind {
		case "local", "external", "migration":
			if s.DeviceID != "" {
				return bad()
			}
		case "device":
			if s.DeviceID == "" || len(s.DeviceID) > 256 || !utf8.ValidString(string(s.DeviceID)) || strings.ContainsRune(string(s.DeviceID), 0) {
				return bad()
			}
		default:
			return bad()
		}
	}
	slices.SortFunc(q.Sources, func(a, b HistorySearchSource) int {
		if a.Kind != b.Kind {
			return strings.Compare(a.Kind, b.Kind)
		}
		return strings.Compare(string(a.DeviceID), string(b.DeviceID))
	})
	q.Sources = slices.Compact(q.Sources)
	if q.CreatedFromMs < 0 || q.CreatedBeforeMs < 0 || (q.CreatedFromMs > 0 && q.CreatedBeforeMs > 0 && q.CreatedFromMs >= q.CreatedBeforeMs) {
		return bad()
	}
	return q, nil
}
func searchKey(v any) string {
	b, _ := json.Marshal(v)
	s := sha256.Sum256(b)
	return hex.EncodeToString(s[:])
}
func (q HistorySearchQuery) toStoreSearch() store.SearchFilter {
	f := store.SearchFilter{HistoryFilter: store.HistoryFilter{Tags: q.Tags, FromMs: q.CreatedFromMs, BeforeMs: q.CreatedBeforeMs}, Terms: q.TextTerms, TagAny: q.TagMatch == "any", Extensions: q.Extensions}
	for _, t := range q.Types {
		f.Types = append(f.Types, string(t))
	}
	for _, s := range q.Sources {
		f.Sources = append(f.Sources, store.SearchSource{Kind: s.Kind, DeviceID: string(s.DeviceID)})
	}
	return f
}
func searchLimit(n int) (int, *Error) {
	if n == 0 {
		return 50, nil
	}
	if n < 1 || n > 200 {
		return 0, newErr(CodeInvalidArgument, "search limit must be between 1 and 200")
	}
	return n, nil
}
func searchError(ctx context.Context, err error) error {
	if ctx.Err() != nil {
		return ctxErr(ctx.Err())
	}
	if errors.Is(err, store.ErrSearchIncomplete) {
		return newErr(CodeUnavailable, "the search index is incomplete")
	}
	return newErr(CodeUnavailable, "cannot read history search")
}
func (e *Engine) SearchHistory(ctx context.Context, req HistorySearchRequest) (HistorySearchPage, error) {
	release, err := e.historyGate(ctx, false, true)
	if err != nil {
		return HistorySearchPage{}, err
	}
	defer release()
	ctx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	q, eerr := normalizeSearch(req.Query)
	if eerr != nil {
		return HistorySearchPage{}, eerr
	}
	limit, eerr := searchLimit(req.Limit)
	if eerr != nil {
		return HistorySearchPage{}, eerr
	}
	key := "search:" + searchKey(q)
	now := historyNow().UnixMilli()
	read := store.SearchRead{Filter: q.toStoreSearch(), NowMs: now, Limit: limit + 1}
	if req.Cursor != "" {
		cp, ok := e.hist.open(req.Cursor)
		switch {
		case !ok:
			return HistorySearchPage{}, newErr(CodeInvalidArgument, "invalid search cursor")
		case cp.Inst != e.instance:
			return HistorySearchPage{}, newErr(CodeResourceExpired, "search cursor belongs to another instance")
		case cp.Filter != key:
			return HistorySearchPage{}, newErr(CodeConflict, "search query changed")
		}
		read.Upper, read.After, read.AfterTime = cp.Upper, cp.After, cp.AfterTime
	}
	hits, rev, newest, complete, serr := e.st.SearchHistory(ctx, read)
	if serr != nil {
		return HistorySearchPage{}, searchError(ctx, serr)
	}
	page := HistorySearchPage{Hits: []HistorySearchHit{}, HistoryRevision: rev, EvaluatedAtMs: now, IndexIncomplete: !complete}
	if read.Upper == 0 {
		read.Upper = newest
	}
	if len(hits) > limit {
		hits = hits[:limit]
		last := hits[limit-1].Item
		page.NextCursor = e.hist.seal(cursorPayload{Inst: e.instance, Filter: key, Upper: read.Upper, After: last.Seq, AfterTime: last.OrderMs})
	}
	for _, h := range hits {
		page.Hits = append(page.Hits, HistorySearchHit{Item: e.historyItem(h.Item), Preview: searchPreview(h.Preview, q.TextTerms)})
	}
	return page, nil
}
func searchPreview(text string, terms []string) HistorySearchPreview {
	p := HistorySearchPreview{Text: text, Highlights: []HistorySearchRange{}}
	r := []rune(strings.ToLower(text))
	for start := 0; start < len(r); start++ {
		for _, t := range terms {
			tr := []rune(t)
			if len(tr) > 0 && start+len(tr) <= len(r) && string(r[start:start+len(tr)]) == t {
				end := start + len(tr)
				if len(p.Highlights) > 0 && start <= p.Highlights[len(p.Highlights)-1].End {
					p.Highlights[len(p.Highlights)-1].End = max(end, p.Highlights[len(p.Highlights)-1].End)
				} else {
					p.Highlights = append(p.Highlights, HistorySearchRange{Start: start, End: end})
				}
			}
		}
	}
	return p
}
func (e *Engine) CountHistorySearches(ctx context.Context, req HistorySearchCountsRequest) (HistorySearchCounts, error) {
	release, err := e.historyGate(ctx, false, true)
	if err != nil {
		return HistorySearchCounts{}, err
	}
	defer release()
	ctx, cancel := context.WithTimeout(ctx, 4*time.Second)
	defer cancel()
	if len(req.Queries) > 32 {
		return HistorySearchCounts{}, newErr(CodeInvalidArgument, "too many search count queries")
	}
	filters := make([]store.SearchFilter, 0, len(req.Queries))
	for _, q := range req.Queries {
		q, eerr := normalizeSearch(q)
		if eerr != nil {
			return HistorySearchCounts{}, eerr
		}
		filters = append(filters, q.toStoreSearch())
	}
	now := historyNow().UnixMilli()
	counts, rev, serr := e.st.CountHistorySearches(ctx, filters, now)
	if serr != nil {
		return HistorySearchCounts{}, searchError(ctx, serr)
	}
	if counts == nil {
		counts = []uint64{}
	}
	return HistorySearchCounts{Counts: counts, HistoryRevision: rev, EvaluatedAtMs: now}, nil
}
func (e *Engine) ListHistorySearchCandidates(ctx context.Context, req HistorySearchCandidatesRequest) (HistorySearchCandidatesPage, error) {
	release, err := e.historyGate(ctx, false, true)
	if err != nil {
		return HistorySearchCandidatesPage{}, err
	}
	defer release()
	ctx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	if (req.Field != "tag" && req.Field != "source" && req.Field != "extension") || len(req.Prefix) > 4096 || !utf8.ValidString(req.Prefix) || strings.ContainsRune(req.Prefix, 0) {
		return HistorySearchCandidatesPage{}, newErr(CodeInvalidArgument, "invalid search catalogue request")
	}
	limit, eerr := searchLimit(req.Limit)
	if eerr != nil {
		return HistorySearchCandidatesPage{}, eerr
	}
	prefix := strings.ToLower(req.Prefix)
	key := "catalogue:" + searchKey([]string{req.Field, prefix})
	after := ""
	if req.Cursor != "" {
		cp, ok := e.hist.open(req.Cursor)
		switch {
		case !ok:
			return HistorySearchCandidatesPage{}, newErr(CodeInvalidArgument, "invalid catalogue cursor")
		case cp.Inst != e.instance:
			return HistorySearchCandidatesPage{}, newErr(CodeResourceExpired, "catalogue cursor belongs to another instance")
		case cp.Filter != key:
			return HistorySearchCandidatesPage{}, newErr(CodeConflict, "catalogue query changed")
		}
		after = cp.SearchAfter
	}
	now := historyNow().UnixMilli()
	vs, rev, serr := e.st.SearchCandidates(ctx, req.Field, prefix, after, limit+1, now)
	if serr != nil {
		return HistorySearchCandidatesPage{}, searchError(ctx, serr)
	}
	page := HistorySearchCandidatesPage{Candidates: []HistorySearchCandidate{}, HistoryRevision: rev, EvaluatedAtMs: now}
	if len(vs) > limit {
		vs = vs[:limit]
		page.NextCursor = e.hist.seal(cursorPayload{Inst: e.instance, Filter: key, SearchAfter: vs[limit-1].Key})
	}
	for _, v := range vs {
		c := HistorySearchCandidate{Value: v.Value, Label: v.Label}
		if v.Kind != "" {
			c.Source = &HistorySearchSource{Kind: v.Kind, DeviceID: DeviceID(v.DeviceID)}
		}
		page.Candidates = append(page.Candidates, c)
	}
	return page, nil
}

// prepareSearch derives only representations and names. It never opens file parts.
func prepareSearch(it *store.HistoryItem, dir string, budget int64) {
	it.SearchBudget = budget
	it.SearchReady = true
	hasPlain := false
	for _, p := range it.Parts {
		hasPlain = hasPlain || (p.Kind != "file" && p.MIME == MimeText)
	}
	for _, p := range it.Parts {
		text, ext := "", ""
		switch {
		case p.Kind == "file":
			text = p.Name
			ext = strings.ToLower(strings.TrimPrefix(filepath.Ext(p.Name), "."))
		case p.MIME == MimePNG:
			ext = "png"
		case p.MIME == MimeText || (!hasPlain && p.MIME == MimeHTML):
			limit := int64(64 << 20)
			if p.MIME == MimeHTML {
				limit = 4 << 20
			}
			if p.Size > limit {
				it.SearchReady = false
				continue
			}
			b := p.Inline
			if b == nil && p.Blob != "" {
				f, err := os.Open(filepath.Join(dir, p.Blob, "data"))
				if err != nil {
					it.SearchReady = false
					continue
				}
				b, err = io.ReadAll(io.LimitReader(f, limit+1))
				_ = f.Close()
				if err != nil {
					it.SearchReady = false
					continue
				}
			}
			if int64(len(b)) != p.Size || int64(len(b)) > limit || !utf8.Valid(b) {
				it.SearchReady = false
				continue
			}
			text = string(b)
			if p.MIME == MimeHTML {
				var err error
				text, err = htmlSearchText(text)
				if err != nil {
					it.SearchReady = false
					continue
				}
			}
		default:
			continue
		}
		it.SearchDocs = append(it.SearchDocs, store.SearchDocument{Text: text, Folded: strings.ToLower(text), Extension: ext})
	}
}
func htmlSearchText(s string) (string, error) {
	root, err := html.Parse(strings.NewReader(s))
	if err != nil {
		return "", err
	}
	var b strings.Builder
	var visit func(*html.Node)
	visit = func(n *html.Node) {
		block := false
		if n.Type == html.ElementNode {
			switch n.Data {
			case "script", "style", "head", "template":
				return
			case "address", "article", "aside", "blockquote", "br", "dd", "div", "dl", "dt", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "li", "main", "nav", "ol", "p", "pre", "section", "table", "td", "th", "tr", "ul":
				block = true
			}
		}
		if block {
			b.WriteByte(' ')
		}
		if n.Type == html.TextNode {
			b.WriteString(n.Data)
		}
		for child := n.FirstChild; child != nil; child = child.NextSibling {
			visit(child)
		}
		if block {
			b.WriteByte(' ')
		}
	}
	visit(root)
	return strings.Join(strings.Fields(b.String()), " "), nil
}
func repairSearch(st *store.Store, dir string, budget int64) error {
	after := int64(0)
	for {
		items, err := st.SearchUnready(after)
		if err != nil {
			return err
		}
		if len(items) == 0 {
			return nil
		}
		for _, it := range items {
			prepareSearch(&it, dir, budget)
			if err = st.RepairSearch(it.Seq, it.SearchDocs, it.SearchReady, budget); err != nil {
				return err
			}
			after = it.Seq
		}
	}
}
