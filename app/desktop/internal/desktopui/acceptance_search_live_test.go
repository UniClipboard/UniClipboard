package desktopui

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// Process scenarios and failure modes precede implementation in the search contract.
// No renderer, native window or system clipboard is started.
func scenarioHistorySearchProcess(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Minute)
	defer cancel()
	root, e := filepath.Abs("../../../..")
	if e != nil {
		t.Fatal(e)
	}
	private, e := os.MkdirTemp("/tmp", "unic-search-")
	if e != nil {
		t.Fatal(e)
	}
	private, e = filepath.EvalSymlinks(private)
	if e != nil {
		t.Fatal(e)
	}
	defer func() {
		if e := os.RemoveAll(private); e != nil {
			t.Error(e)
		}
	}()
	hash := func(b []byte) string { s := sha256.Sum256(b); return hex.EncodeToString(s[:]) }
	sources := map[string]string{}
	for _, dir := range []string{"engine", "internal/store", "internal/localapi", "app/service", "app/desktop"} {
		if e = filepath.WalkDir(filepath.Join(root, dir), func(p string, d os.DirEntry, e error) error {
			if e != nil {
				return e
			}
			if d.IsDir() || !strings.HasSuffix(p, ".go") {
				return nil
			}
			b, e := os.ReadFile(p)
			if e != nil {
				return e
			}
			rel, _ := filepath.Rel(root, p)
			sources[rel] = hash(b)
			return nil
		}); e != nil {
			t.Fatal(e)
		}
	}
	checks := []string{}
	succeeded := false
	receipt := map[string]any{"source_sha256": sources, "native_ui": false, "renderer": false}
	defer func() {
		receipt["passed"] = succeeded && !t.Failed()
		receipt["checks"] = checks
		if path := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"); path != "" {
			b, _ := json.MarshalIndent(receipt, "", "  ")
			if e := os.MkdirAll(filepath.Dir(path), 0700); e != nil {
				t.Error(e)
			}
			if e := os.WriteFile(path, b, 0600); e != nil {
				t.Error(e)
			}
		}
	}()
	exe := filepath.Join(private, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if out, e := build.CombinedOutput(); e != nil {
		t.Fatalf("build: %v\n%s", e, out)
	}
	binary, e := os.ReadFile(exe)
	if e != nil {
		t.Fatal(e)
	}
	receipt["unicd_sha256"] = hash(binary)
	home := filepath.Join(private, "home")
	if e = os.MkdirAll(filepath.Join(home, "inbox", "file-cache"), 0700); e != nil {
		t.Fatal(e)
	}
	// Offline import is a distinct production insertion path; reopen the same profile afterwards.
	importer, e := engine.New(ctx, engine.Options{DataDir: home, MigrationMode: true})
	if e != nil {
		t.Fatal(e)
	}
	imported := "Imported migration marker"
	_, e = importer.ImportHistory(ctx, engine.HistoryImportRequest{Source: "acceptance", RecordID: "1", CreatedAtMs: time.Now().Add(-48 * time.Hour).UnixMilli(), Content: engine.ContentInput{Representations: []engine.RepresentationInput{{MIME: "text/plain", Size: int64(len(imported)), Reader: strings.NewReader(imported)}}}})
	if e != nil {
		t.Fatal(e)
	}
	if e = importer.Close(ctx); e != nil {
		t.Fatal(e)
	}
	if e = os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600); e != nil {
		t.Fatal(e)
	}
	env := []string{}
	for _, v := range os.Environ() {
		if !strings.HasPrefix(v, "UNIC_") {
			env = append(env, v)
		}
	}
	var proc *exec.Cmd
	var logs bytes.Buffer
	start := func() {
		t.Helper()
		proc = exec.CommandContext(ctx, exe)
		proc.Env = append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
		proc.Stdout = &logs
		proc.Stderr = &logs
		if e := proc.Start(); e != nil {
			t.Fatal(e)
		}
	}
	stop := func() {
		if proc != nil {
			_ = proc.Process.Kill()
			_ = proc.Wait()
			proc = nil
		}
	}
	defer stop()
	start()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	wait := func(label string, f func() bool) {
		t.Helper()
		deadline := time.Now().Add(25 * time.Second)
		for time.Now().Before(deadline) {
			if f() {
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
		t.Fatalf("timeout: %s", label)
	}
	wait("service", func() bool { _, e := client.Status(ctx); return e == nil })
	submit := func(key, text, format string) string {
		t.Helper()
		out, e := client.Submit(ctx, api.SubmitInput{RequestID: key, Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: format, Data: []byte(text)}}}})
		if e != nil {
			t.Fatal(e)
		}
		return string(out.HistoryID)
	}
	search := func(q engine.HistorySearchQuery) api.HistorySearchPage {
		t.Helper()
		page, e := client.SearchHistory(ctx, engine.HistorySearchRequest{Query: q, Limit: 200})
		if e != nil {
			t.Fatal(e)
		}
		return page
	}
	requireIDs := func(q engine.HistorySearchQuery, want ...string) {
		t.Helper()
		page := search(q)
		got := []string{}
		for _, h := range page.Hits {
			got = append(got, h.Item.ID)
		}
		if len(got) != len(want) {
			t.Fatalf("result count: got %d want %d", len(got), len(want))
		}
		for _, id := range want {
			found := false
			for _, g := range got {
				found = found || g == id
			}
			if !found {
				t.Fatal("expected record missing")
			}
		}
	}
	long := strings.Repeat("padding ", 40000) + "终点产品讨论 CAFÉ 😀 100% _tag AND a\"b"
	longID := submit("long", long, "text/plain")
	separated := submit("separated", "foo ... bar", "text/plain")
	phrase := submit("phrase", "foo bar", "text/plain")
	htmlID := submit("html", "<html><head><title>head-only-secret</title></head><body><script>hidden-script-secret</script><style>hidden-style-secret</style><p>HTML visible product</p></body></html>", "text/html")
	for n := 0; n < 55; n++ {
		submit(fmt.Sprintf("page-%d", n), fmt.Sprintf("unrelated row %d", n), "text/plain")
	}
	for _, term := range []string{"产", "产品", "产品讨", "café", "😀", "100%", "_tag", "AND", "a\"b"} {
		requireIDs(engine.HistorySearchQuery{TextTerms: []string{term}}, longID)
	}
	var full bytes.Buffer
	if e = client.CopyHistoryContent(ctx, longID, 0, &full); e != nil {
		t.Fatal(e)
	}
	if hash(full.Bytes()) != hash([]byte(long)) {
		t.Fatal("payload hash mismatch")
	}
	checks = append(checks, "global short-CJK/literal/Unicode search finds long old content beyond previews; exact payload hash verified")
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"foo", "bar"}}, separated, phrase)
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"foo bar"}}, phrase)
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"HTML visible"}, Types: []engine.HistorySearchType{"richtext"}}, htmlID)
	for _, term := range []string{"hidden-script-secret", "hidden-style-secret", "head-only-secret"} {
		requireIDs(engine.HistorySearchQuery{TextTerms: []string{term}})
	}
	migrationPage := search(engine.HistorySearchQuery{TextTerms: []string{"migration marker"}})
	if len(migrationPage.Hits) != 1 {
		t.Fatal("offline import missing")
	}
	requireIDs(engine.HistorySearchQuery{Sources: []engine.HistorySearchSource{{Kind: "migration"}}, TextTerms: []string{"migration marker"}}, migrationPage.Hits[0].Item.ID)
	malformedID := submit("html-fragment", "<head><title>hidden-fragment-title</title><body><p>fixed-fragment-needle</p>", "text/html")
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"fixed-fragment-needle"}}, malformedID)
	inlineID := submit("html-inline", "<p>spanjoin<strong>case</strong></p>", "text/html")
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"spanjoincase"}}, inlineID)
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"hidden-fragment-title"}})
	checks = append(checks, "phrases and terms differ; HTML fallback omits scripts/styles/head; offline imports are indexed")
	// Tag-mode differences must be explicit and counts must use exactly the same predicate.
	for id, tags := range map[string][]string{longID: {"work", "design"}, phrase: {"work"}, separated: {"design"}} {
		it, e := client.GetHistory(ctx, id)
		if e != nil {
			t.Fatal(e)
		}
		if _, e = client.UpdateHistory(ctx, id, api.HistoryUpdate{Revision: it.Revision, Tags: &tags}); e != nil {
			t.Fatal(e)
		}
	}
	all := engine.HistorySearchQuery{Tags: []string{"work", "design"}, TagMatch: "all"}
	any := all
	any.TagMatch = "any"
	requireIDs(all, longID)
	requireIDs(any, longID, phrase, separated)
	counts, e := client.CountHistorySearches(ctx, engine.HistorySearchCountsRequest{Queries: []engine.HistorySearchQuery{all, any, {TextTerms: []string{"foo bar"}}}})
	if e != nil {
		t.Fatal(e)
	}
	if !reflect.DeepEqual(counts.Counts, []uint64{1, 3, 1}) {
		t.Fatal("prospective counts differ", counts.Counts)
	}
	checks = append(checks, "all/any tags and batch counts match complete prospective queries")
	// A bundle matches once, and a phrase cannot span two stored names.
	path1 := filepath.Join(private, "alpha.txt")
	path2 := filepath.Join(private, "beta.png")
	for _, p := range []string{path1, path2} {
		if e = os.WriteFile(p, []byte("not-searchable-file-body"), 0600); e != nil {
			t.Fatal(e)
		}
	}
	sent, e := client.Submit(ctx, api.SubmitInput{RequestID: "files", Snapshot: clip.Snapshot{Files: []clip.File{{Name: "alpha.txt", Path: path1, Size: 24}, {Name: "beta.png", Path: path2, Size: 24}}}})
	if e != nil {
		t.Fatal(e)
	}
	fileID := string(sent.HistoryID)
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"alpha", "beta"}}, fileID)
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"alpha.txt\nbeta.png"}})
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"not-searchable-file-body"}})
	requireIDs(engine.HistorySearchQuery{Extensions: []string{"txt", "png"}}, fileID)
	candidates, e := client.ListHistorySearchCandidates(ctx, engine.HistorySearchCandidatesRequest{Field: "extension", Limit: 200})
	if e != nil || len(candidates.Candidates) != 2 {
		t.Fatal("extension catalogue", e)
	}
	candidates, e = client.ListHistorySearchCandidates(ctx, engine.HistorySearchCandidatesRequest{Field: "tag", Prefix: "wor", Limit: 200})
	if e != nil || len(candidates.Candidates) != 1 || candidates.Candidates[0].Value != "work" {
		t.Fatal("tag catalogue", e)
	}
	checks = append(checks, "file-name/extension search uses stored basenames without body search; catalogues contain real values")
	page := search(engine.HistorySearchQuery{TextTerms: []string{"产品"}})
	for _, hit := range page.Hits {
		rr := []rune(hit.Preview.Text)
		if len(rr) > 512 || len(hit.Preview.Highlights) == 0 {
			t.Fatal("preview bounds/highlights")
		}
		for _, r := range hit.Preview.Highlights {
			if r.Start < 0 || r.End > len(rr) || r.Start >= r.End {
				t.Fatal("bad rune range")
			}
		}
	}
	checks = append(checks, "bounded previews carry valid rune highlight ranges")
	first, e := client.SearchHistory(ctx, engine.HistorySearchRequest{Limit: 10})
	if e != nil || first.NextCursor == "" {
		t.Fatal("missing cursor", e)
	}
	if _, e = client.SearchHistory(ctx, engine.HistorySearchRequest{Query: all, Cursor: first.NextCursor}); api.ReasonOf(e) != "conflict" {
		t.Fatal("cursor not bound to full query", e)
	}
	if _, e = client.SearchHistory(ctx, engine.HistorySearchRequest{Cursor: first.NextCursor + "x"}); api.ReasonOf(e) != "invalid_argument" {
		t.Fatal("tampered cursor accepted", e)
	}
	cancelled, cc := context.WithCancel(ctx)
	cc()
	if _, e = client.SearchHistory(cancelled, engine.HistorySearchRequest{}); e == nil {
		t.Fatal("cancelled search succeeded")
	}
	checks = append(checks, "signed cursors reject changed queries and tampering; cancellation is visible")
	// Search projection changes must reach both existing desktop editor models without rendering.
	desktop := NewDesktop(ctx, client)
	defer desktop.Close()
	d := desktop.d
	// Exercise production content reads through the real service without a renderer.
	wait("prefetch metadata", func() bool { d.live.poll(d); return d.live.loaded && !d.live.loading && len(d.entries) == 50 })
	l := d.live
	window := func(first, last int) {
		t.Helper()
		d.selectEntry(d.entries[first].id)
		l.prefetchDetails(d, d.entries, first, last)
		wait("prefetch window", func() bool {
			l.poll(d)
			if len(l.detailReads) > detailReadLimit {
				t.Fatal("content read concurrency exceeds bound")
			}
			for i := max(0, first-10); i <= min(len(d.entries)-1, last+10); i++ {
				if _, ok := l.details[d.entries[i].id]; !ok {
					return false
				}
			}
			return len(l.detailReads) == 0
		})
	}
	window(20, 39)
	if len(l.details) != 40 {
		t.Fatalf("20 visible plus ten each side: got %d", len(l.details))
	}
	serialBefore := l.contentSerial
	for i := 20; i <= 39; i++ {
		d.selectEntry(d.entries[i].id)
		l.loadDetail(d)
		if l.detailBusy || d.current().text == "" {
			t.Fatal("prefetched selection is not immediately ready")
		}
	}
	if l.contentSerial != serialBefore || len(l.detailReads) != 0 {
		t.Fatal("cached selection reread content")
	}
	window(40, 49)
	if len(l.details) != 20 {
		t.Fatalf("bottom window retained %d details", len(l.details))
	}
	for i := 0; i < 30; i++ {
		v := d.entries[i]
		if v.text != "" || v.image != nil || v.rich != nil || v.folder != nil {
			t.Fatal("evicted entry retains payload")
		}
	}
	window(0, 19)
	if len(l.details) != 30 {
		t.Fatal("reverse scroll did not refill top window")
	}
	// Keep the active detail while its list row leaves the prefetch window.
	d.selectEntry(d.entries[0].id)
	l.prefetchDetails(d, d.entries, 40, 49)
	wait("active detail retained", func() bool { l.poll(d); return len(l.detailReads) == 0 && len(l.details) == 21 })
	if d.current().text == "" {
		t.Fatal("scrolling removed the active detail")
	}
	// Reconciliation must preserve ready content and must not refetch it.
	serialBefore = l.contentSerial
	appliedBefore := l.appliedSerial
	l.nextRefresh = time.Time{}
	wait("prefetch reconciliation", func() bool { l.poll(d); return l.appliedSerial > appliedBefore && !l.loading })
	if l.contentSerial != serialBefore || len(l.detailReads) != 0 {
		t.Fatal("unchanged reconciliation reread details")
	}
	// Leave active reads outstanding before replacing the search generation.
	l.prefetchDetails(d, d.entries, 40, 49)
	checks = append(checks, "20 visible details plus ten per side are prefetched; cached selection is immediate without rereads; scrolling evicts and reverse scrolling refills payloads")
	wait("desktop query", func() bool {
		d.query = "产品"
		d.search.buffer = "产品"
		d.live.poll(d)
		return !d.live.loading && len(d.entries) == 1 && d.entries[0].id == longID
	})
	for id := range l.details {
		if id != longID {
			t.Fatal("obsolete query payload survived")
		}
	}
	l.prefetchDetails(d, d.entries, 0, 0)
	wait("search detail prefetch", func() bool { l.poll(d); _, ok := l.details[longID]; return ok && len(l.detailReads) == 0 })
	serialBefore = l.contentSerial
	l.loadDetail(d)
	if l.detailBusy || l.contentSerial != serialBefore {
		t.Fatal("search result reread prefetched detail")
	}
	checks = append(checks, "search results reuse viewport prefetch and reject prior-query completions")
	checks = append(checks, "main desktop uses global content search rather than page filtering")
	quick := newLiveQuick(ctx, client, "system", nil)
	defer quick.live.d.live.cancel()
	wait("quick prefetch metadata", func() bool { quick.live.poll(quick); return len(quick.rows) == 50 && !quick.live.d.live.loading })
	ql := quick.live.d.live
	ql.prefetchDetails(quick.live.d, quick.live.d.entries, 20, 39)
	wait("quick prefetch", func() bool { quick.live.poll(quick); return len(ql.details) >= 40 && len(ql.detailReads) == 0 })
	quick.hovered = 22
	quick.previewID = quick.rows[22].id
	quick.live.poll(quick)
	if ql.detailBusy || quick.live.d.current().text == "" || quick.hovered != 22 {
		t.Fatal("quick prefetched preview was not immediately ready")
	}
	checks = append(checks, "quick-panel results use the same prefetch cache; ready previews preserve hover and avoid loading")
	quick.query = "foo bar"
	quick.applySearch()
	wait("quick phrase", func() bool {
		quick.live.poll(quick)
		return !quick.live.d.live.loading && len(quick.rows) == 1 && quick.rows[0].id == phrase
	})
	quick.query = ""
	quick.chips = []qpFilter{{dimension: "tag", value: "work"}, {dimension: "tag", value: "design"}}
	quick.applySearch()
	wait("quick union", func() bool { quick.live.poll(quick); return !quick.live.d.live.loading && len(quick.rows) == 3 })
	quick.query = "no-such-content"
	quick.applySearch()
	quick.query = "foo bar"
	quick.applySearch()
	wait("quick stale generation", func() bool {
		quick.live.poll(quick)
		return !quick.live.d.live.loading && len(quick.rows) == 1 && quick.rows[0].id == phrase
	})
	checks = append(checks, "quick-panel phrase/union queries use real global search and discard obsolete generations")
	serial := quick.live.d.live.appliedSerial
	quick.live.d.live.nextRefresh = time.Time{}
	quick.live.poll(quick)
	if quick.live.d.live.foregroundLoading || len(quick.rows) != 1 || quick.rows[0].id != phrase {
		t.Fatal("quick-panel background reconciliation replaced existing search results")
	}
	wait("quick silent reconciliation", func() bool {
		quick.live.poll(quick)
		return quick.live.d.live.appliedSerial > serial && !quick.live.d.live.loading
	})
	checks = append(checks, "quick-panel background reconciliation preserves results without foreground loading")
	// Fresh candidate pages, limit validation, expiry and Action commits use the same read surface.
	catalogueFirst, e := client.ListHistorySearchCandidates(ctx, engine.HistorySearchCandidatesRequest{Field: "tag", Limit: 1})
	if e != nil || catalogueFirst.NextCursor == "" {
		t.Fatal("catalogue pagination", e)
	}
	if _, e = client.ListHistorySearchCandidates(ctx, engine.HistorySearchCandidatesRequest{Field: "source", Cursor: catalogueFirst.NextCursor}); api.ReasonOf(e) != "conflict" {
		t.Fatal("catalogue cursor not scoped", e)
	}
	invalid := engine.HistorySearchQuery{TextTerms: make([]string, 17)}
	if _, e = client.SearchHistory(ctx, engine.HistorySearchRequest{Query: invalid}); api.ReasonOf(e) != "invalid_argument" {
		t.Fatal("term limit not enforced", e)
	}
	expiringID := submit("expires", "expiring-specific-needle", "text/plain")
	expiring, e := client.GetHistory(ctx, expiringID)
	if e != nil {
		t.Fatal(e)
	}
	expires := time.Now().Add(500 * time.Millisecond).UnixMilli()
	if _, e = client.UpdateHistory(ctx, expiringID, api.HistoryUpdate{Revision: expiring.Revision, ExpiresAtMs: &expires}); e != nil {
		t.Fatal(e)
	}
	wait("expiry search", func() bool {
		return len(search(engine.HistorySearchQuery{TextTerms: []string{"expiring-specific-needle"}}).Hits) == 0
	})
	actionSource := submit("action-source", "action-specific-needle", "text/plain")
	action, e := client.ExecuteAction(ctx, actionSource, "builtin.text/upper", api.ActionInput{RequestID: "search-action"})
	if e != nil {
		t.Fatal(e)
	}
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"action-specific-needle"}}, actionSource, action.Item.ID)
	checks = append(checks, "catalogue cursors/limits, immediate expiry visibility and Action indexing verified")
	// Inject a transaction failure through a private profile, never a user database.
	inspect, e := sql.Open("sqlite", "file:"+filepath.Join(home, "unic.db"))
	if e != nil {
		t.Fatal(e)
	}
	defer func() { _ = inspect.Close() }()
	if _, e = inspect.Exec(`CREATE TRIGGER reject_search_fixture BEFORE INSERT ON history_search_docs WHEN new.body='rollback-specific-needle' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END`); e != nil {
		t.Fatal(e)
	}
	_, e = client.Submit(ctx, api.SubmitInput{RequestID: "rollback", Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: "text/plain", Data: []byte("rollback-specific-needle")}}}})
	if e == nil || strings.Contains(e.Error(), "rollback-specific-needle") {
		t.Fatal("failed commit was accepted or leaked content", e)
	}
	if _, e = inspect.Exec(`DROP TRIGGER reject_search_fixture`); e != nil {
		t.Fatal(e)
	}
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"rollback-specific-needle"}})
	checks = append(checks, "search-index insertion failure rolls back history and redacts private data")
	// Counts use a read connection while an ordinary production submission writes.
	batch := make([]engine.HistorySearchQuery, 32)
	for i := range batch {
		batch[i].TextTerms = []string{string(rune(0x4e00 + i))}
	}
	began := make(chan struct{})
	finished := make(chan error, 1)
	countStart := time.Now()
	go func() {
		close(began)
		_, err := client.CountHistorySearches(ctx, engine.HistorySearchCountsRequest{Queries: batch})
		finished <- err
	}()
	<-began
	time.Sleep(time.Millisecond)
	writeStart := time.Now()
	submit("during-count", "writer-specific-needle", "text/plain")
	writeDuration := time.Since(writeStart)
	if e = <-finished; e != nil {
		t.Fatal(e)
	}
	receipt["concurrent_count_ms"] = time.Since(countStart).Milliseconds()
	receipt["concurrent_write_ms"] = writeDuration.Milliseconds()
	if writeDuration > time.Second {
		t.Fatal("count blocked submission beyond the fixture budget")
	}
	checks = append(checks, "batch count and submission run concurrently; small-fixture write stays below one second")
	// A deliberate low derived-index budget preserves history and reports incompleteness.
	quotaHome := filepath.Join(private, "quota")
	if e = os.MkdirAll(filepath.Join(quotaHome, "inbox", "file-cache"), 0700); e != nil {
		t.Fatal(e)
	}
	quota, e := engine.New(ctx, engine.Options{DataDir: quotaHome, MigrationMode: true, History: engine.HistoryOptions{MaxBytes: 1024}})
	if e != nil {
		t.Fatal(e)
	}
	defer func() { _ = quota.Close(context.Background()) }()
	quotaText := strings.Repeat("q", 128)
	if _, e = quota.ImportHistory(ctx, engine.HistoryImportRequest{Source: "quota", RecordID: "1", Content: engine.ContentInput{Representations: []engine.RepresentationInput{{MIME: "text/plain", Size: 128, Reader: strings.NewReader(quotaText)}}}}); e != nil {
		t.Fatal(e)
	}
	if _, e = quota.SearchHistory(ctx, engine.HistorySearchRequest{Query: engine.HistorySearchQuery{TextTerms: []string{"q"}}}); api.ReasonOf(e) != "unavailable" {
		t.Fatal("incomplete index returned partial search", e)
	}
	quotaPage, e := quota.ListHistory(ctx, engine.ListHistoryRequest{})
	if e != nil || len(quotaPage.Items) != 1 {
		t.Fatal("index quota lost history", e)
	}
	if e = quota.Close(ctx); e != nil {
		t.Fatal(e)
	}
	checks = append(checks, "derived-index quota preserves history and rejects incomplete text search")

	it, e := client.GetHistory(ctx, longID)
	if e != nil {
		t.Fatal(e)
	}
	if e = client.DeleteHistory(ctx, longID, it.Revision); e != nil {
		t.Fatal(e)
	}
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"产品"}})
	stop()
	// Rebuild only missing derived state from persisted authoritative content.
	if _, e = inspect.Exec(`DELETE FROM history_search_docs WHERE item_seq=(SELECT seq FROM clip_items WHERE id=?)`, phrase); e != nil {
		t.Fatal(e)
	}
	if _, e = inspect.Exec(`DELETE FROM history_search_state WHERE item_seq=(SELECT seq FROM clip_items WHERE id=?)`, phrase); e != nil {
		t.Fatal(e)
	}
	start()
	wait("restart", func() bool { _, e := client.Status(ctx); return e == nil })
	requireIDs(engine.HistorySearchQuery{TextTerms: []string{"foo bar"}}, phrase)
	if _, e = client.SearchHistory(ctx, engine.HistorySearchRequest{Cursor: first.NextCursor}); api.ReasonOf(e) != "resource_expired" {
		t.Fatal("cursor survived instance change", e)
	}
	checks = append(checks, "deletion removes search hits; missing projection repairs on reopen; old-instance cursors expire")
	stop()
	for _, secret := range []string{"rollback-specific-needle", "hidden-script-secret", "writer-specific-needle", "alpha.txt"} {
		if strings.Contains(logs.String(), secret) {
			t.Fatal("search content leaked into service logs")
		}
	}
	checks = append(checks, "service logs omit query/body/file-name sentinels")
	var dbBytes int64
	for _, suffix := range []string{"", "-wal", "-shm"} {
		if st, e := os.Stat(filepath.Join(home, "unic.db") + suffix); e == nil {
			dbBytes += st.Size()
		}
	}
	receipt["database_and_wal_bytes"] = dbBytes
	succeeded = true
	// Keep stdout/log content private. Only the receipt survives the test.
}
