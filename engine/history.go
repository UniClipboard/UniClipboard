package engine

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/store"
)

const (
	historyDirName    = "history"
	historyInlineMax  = 256 << 10
	historyDefaultLim = 50
	historyMaxLim     = 200
	historyMaxTags    = 32
	cursorKeyMeta     = "history_cursor_key"
)

// blobTracker keeps a history blob on disk while content retained from
// history still reads it, even after the record was deleted. It is a leaf
// lock: nothing else is acquired while holding mu.
type blobTracker struct {
	dir     string
	mu      sync.Mutex
	holders map[string]int
	doomed  map[string]bool
}

func (b *blobTracker) path(blob string) string { return filepath.Join(b.dir, blob) }

func (b *blobTracker) hold(names []string) {
	b.mu.Lock()
	for _, n := range names {
		b.holders[n]++
	}
	b.mu.Unlock()
}

func (b *blobTracker) release(names []string) {
	b.mu.Lock()
	defer b.mu.Unlock()
	for _, n := range names {
		if b.holders[n]--; b.holders[n] <= 0 {
			delete(b.holders, n)
			if b.doomed[n] {
				delete(b.doomed, n)
				_ = os.RemoveAll(b.path(n))
			}
		}
	}
}

// drop is called after the database no longer references the blobs.
func (b *blobTracker) drop(names []string) {
	b.mu.Lock()
	defer b.mu.Unlock()
	for _, n := range names {
		if b.holders[n] > 0 {
			b.doomed[n] = true
		} else {
			_ = os.RemoveAll(b.path(n))
		}
	}
}

type history struct {
	mu   sync.Mutex // serializes history changes with blob bookkeeping
	opts HistoryOptions
	dir  string
	blob *blobTracker
	key  []byte // cursor MAC key, persisted so cursors verify across restarts
}

func (o HistoryOptions) withDefaults(maxStored int64) (HistoryOptions, *Error) {
	if o.MaxItems < 0 || o.MaxBytes < 0 || o.DefaultTTL < 0 {
		return o, newErr(CodeInvalidArgument, "history options must not be negative")
	}
	if o.MaxItems == 0 {
		o.MaxItems = 10000
	}
	if o.MaxBytes == 0 {
		o.MaxBytes = 4 << 30
		if o.MaxBytes > maxStored {
			o.MaxBytes = maxStored
		}
	}
	if o.MaxBytes > maxStored {
		return o, newErr(CodeInvalidArgument, "history MaxBytes must not exceed MaxStoredBytes")
	}
	return o, nil
}

// openHistory prepares the history directory and removes blobs no record
// references (leftovers of an interrupted commit). Caller holds the data lock.
func openHistory(dataDir string, st *store.Store, opts HistoryOptions) (*history, error) {
	dir := filepath.Join(dataDir, historyDirName)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, err
	}
	key, err := st.Meta(cursorKeyMeta)
	if err != nil {
		return nil, err
	}
	if len(key) == 0 {
		key = []byte(randHex(32))
		if err := st.SetMeta(cursorKeyMeta, key); err != nil {
			return nil, err
		}
	}
	refs, err := st.ReferencedBlobs()
	if err != nil {
		return nil, err
	}
	if ents, err := os.ReadDir(dir); err == nil {
		for _, ent := range ents {
			if !refs[ent.Name()] {
				_ = os.RemoveAll(filepath.Join(dir, ent.Name()))
			}
		}
	}
	return &history{opts: opts, dir: dir, key: key,
		blob: &blobTracker{dir: dir, holders: map[string]int{}, doomed: map[string]bool{}}}, nil
}

// ---- mapping ----

func historyKindOf(t string) HistoryKind {
	switch t {
	case "files":
		return HistoryFiles
	case "image":
		return HistoryImage
	}
	return HistoryText
}

func toHistoryItem(it store.HistoryItem) HistoryItem {
	out := HistoryItem{
		ID: HistoryID(it.ID), Revision: it.Revision, Kind: HistoryKind(it.Kind),
		Origin:    HistoryOrigin{Kind: HistoryOriginKind(it.OriginKind), DeviceID: DeviceID(it.DeviceID), PluginID: PluginID(it.PluginID)},
		SourceApp: it.SourceApp, AutomationDepth: uint32(it.Depth),
		CreatedAtMs: it.CreatedMs, UpdatedAtMs: it.UpdatedMs, ExpiresAtMs: it.ExpiresMs,
		Tags: append([]string{}, it.Tags...), TotalBytes: it.TotalBytes, SyncState: HistorySyncState(it.SyncState),
		Parts: make([]HistoryPartSummary, len(it.Parts)),
	}
	for i, p := range it.Parts {
		k := PartRepresentation
		if p.Kind == "file" {
			k = PartFile
		}
		out.Parts[i] = HistoryPartSummary{Kind: k, MIME: p.MIME, Size: p.Size}
	}
	return out
}

func normalizeTags(tags []string) ([]string, *Error) {
	if len(tags) > historyMaxTags*4 { // refuse absurd input before deduplicating
		return nil, newErr(CodeInvalidArgument, "too many tags")
	}
	seen := map[string]bool{}
	out := []string{}
	for _, t := range tags {
		if t == "" || len(t) > 64 || !utf8.ValidString(t) {
			return nil, newErr(CodeInvalidArgument, "a tag must be 1..64 bytes of valid UTF-8")
		}
		if !seen[t] {
			seen[t] = true
			out = append(out, t)
		}
	}
	if len(out) > historyMaxTags {
		return nil, newErr(CodeInvalidArgument, "at most 32 tags")
	}
	sort.Strings(out) // byte order
	return out, nil
}

func historyStoreErr(err error) *Error {
	switch {
	case errors.Is(err, store.ErrNotFound):
		return newErr(CodeNotFound, "no such history item")
	case errors.Is(err, store.ErrConflict):
		return newErr(CodeConflict, "the history item changed; reload it")
	}
	return retryable(newErr(CodeUnavailable, "the history database failed"))
}

// ---- commit-time persistence ----

// writeBlob copies one part into a new blob directory. The part is opened
// through openPart, so a referenced file that changed fails as source_changed.
func (e *Engine) writeBlob(ctx context.Context, p *partRec, fname string) (string, *Error) {
	h := e.hist
	rc, oerr := e.openPart(ctx, p)
	if oerr != nil {
		return "", oerr
	}
	defer func() { _ = rc.Close() }()
	name := randHex(16)
	tmp := filepath.Join(h.dir, "tmp-"+name)
	if err := os.Mkdir(tmp, 0o700); err != nil {
		return "", newErr(CodeUnavailable, "cannot store the history item")
	}
	if eerr := e.copyIn(ctx, filepath.Join(tmp, fname), rc, p.desc.Size); eerr != nil {
		_ = os.RemoveAll(tmp)
		if eerr.Code == CodeInvalidArgument { // the source did not match its declared size
			return "", newErr(CodeSourceChanged, "the file changed while it was saved")
		}
		return "", eerr
	}
	if err := os.Rename(tmp, filepath.Join(h.dir, name)); err != nil {
		_ = os.RemoveAll(tmp)
		return "", newErr(CodeUnavailable, "cannot store the history item")
	}
	return name, nil
}

func persistentRef(p *partRec) string {
	if ps, ok := p.src.(PersistentFileSource); ok {
		return ps.PersistentRef()
	}
	return ""
}

// persistHistory commits a processed item to the history and returns its ID.
// Blobs are written first and the record second, so a failure leaves earlier
// history intact; eviction happens only after the new record exists.
func (e *Engine) persistHistory(ctx context.Context, pc *processedContent) (HistoryID, *Error) {
	h := e.hist
	c := pc.c
	var stored int64 // what history itself would store; referenced files are free
	for _, p := range c.parts {
		if p.desc.Kind != PartFile || persistentRef(p) == "" {
			stored += p.desc.Size
		}
	}
	if stored > h.opts.MaxBytes {
		return "", newErr(CodeResourceExhausted, "the item is larger than the history allows")
	}
	now := time.Now()
	it := store.HistoryItem{
		ID: randHex(16), Kind: string(historyKindOf(pc.item.Type)), OriginKind: pc.item.Origin.kind,
		DeviceID: string(pc.item.Origin.deviceID), PluginID: pc.item.Origin.pluginID,
		Depth: int(pc.item.AutomationDepth), CreatedMs: now.UnixMilli(), TotalBytes: c.total,
	}
	switch {
	case pc.policy.ExpireAfter > 0:
		it.ExpiresMs = now.Add(pc.policy.ExpireAfter).UnixMilli()
	case h.opts.DefaultTTL > 0:
		it.ExpiresMs = now.Add(h.opts.DefaultTTL).UnixMilli()
	}
	tags, terr := normalizeTags(pc.policy.Tags)
	if terr != nil {
		return "", terr
	}
	it.Tags = tags
	var written []string
	fail := func(err *Error) (HistoryID, *Error) {
		for _, b := range written {
			_ = os.RemoveAll(filepath.Join(h.dir, b))
		}
		return "", err
	}
	for _, p := range c.parts {
		sp := store.HistoryPart{Kind: "rep", MIME: p.desc.MIME, Size: p.desc.Size}
		switch {
		case p.desc.Kind == PartFile && persistentRef(p) != "":
			// The host keeps this file: remember where, copy nothing.
			sp.Kind, sp.Name, sp.MIME = "file", p.desc.Name, p.desc.MIME
			sp.Ref, sp.Version = persistentRef(p), p.meta.Version
		case p.desc.Kind == PartFile:
			sp.Kind, sp.Name, sp.MIME = "file", p.desc.Name, p.desc.MIME
			b, err := e.writeBlob(ctx, p, p.desc.Name)
			if err != nil {
				return fail(err)
			}
			written, sp.Blob = append(written, b), b
		case p.desc.Size <= historyInlineMax && p.data != nil:
			sp.Inline = slices.Clone(p.data)
		default:
			b, err := e.writeBlob(ctx, p, "data")
			if err != nil {
				return fail(err)
			}
			written, sp.Blob = append(written, b), b
		}
		it.Parts = append(it.Parts, sp)
	}
	if err := ctx.Err(); err != nil {
		return fail(ctxErr(err))
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	seq, _, err := e.st.InsertHistory(it)
	if err != nil {
		return fail(historyStoreErr(err))
	}
	e.evictHistoryLocked(seq)
	return HistoryID(it.ID), nil
}

// evictHistoryLocked drops the oldest records (never keep) until the limits
// hold. Failures are retried at the next commit. h.mu held.
func (e *Engine) evictHistoryLocked(keep int64) {
	h := e.hist
	for {
		n, bytes, err := e.st.HistoryTotals()
		if err != nil || (n <= h.opts.MaxItems && bytes <= h.opts.MaxBytes) {
			return
		}
		old, err := e.st.OldestHistory(1)
		if err != nil || len(old) == 0 || old[0].Seq == keep {
			return
		}
		blobs, _, err := e.st.DeleteHistory(old[0].ID, 0)
		if err != nil {
			return
		}
		h.dropUnreferenced(e.st, blobs)
	}
}

// dropUnreferenced removes blobs no record shares any more.
func (h *history) dropUnreferenced(st *store.Store, blobs []string) {
	var free []string
	for _, b := range blobs {
		if in, err := st.BlobInUse(b); err == nil && !in {
			free = append(free, b)
		}
	}
	h.blob.drop(free)
}

// ---- queries ----

// historyReadable rejects calls the lifecycle forbids. e.mu is taken briefly.
func (e *Engine) historyGate(ctx context.Context, mutate, content bool) (func(), *Error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return nil, gerr
	}
	e.mu.Lock()
	life := e.life
	e.mu.Unlock()
	switch {
	case mutate && life != LifecycleCreated && life != LifecycleRunning,
		content && (life == LifecycleStopping || life == LifecycleStarting):
		release()
		return nil, newErr(CodeInvalidState, "the engine is not in a state that allows this")
	}
	return release, nil
}

func (e *Engine) GetItem(ctx context.Context, id HistoryID) (HistoryItem, error) {
	release, gerr := e.historyGate(ctx, false, false)
	if gerr != nil {
		return HistoryItem{}, gerr
	}
	defer release()
	it, err := e.st.GetHistory(string(id), time.Now().UnixMilli(), false)
	if err != nil {
		return HistoryItem{}, historyStoreErr(err)
	}
	return toHistoryItem(it), nil
}

type cursorPayload struct {
	Inst   InstanceID `json:"i"`
	Filter string     `json:"f"`
	Upper  int64      `json:"u"`
	After  int64      `json:"a"`
}

func (h *history) seal(p cursorPayload) string {
	b, _ := json.Marshal(p)
	m := hmac.New(sha256.New, h.key)
	m.Write(b)
	enc := base64.RawURLEncoding
	return enc.EncodeToString(b) + "." + enc.EncodeToString(m.Sum(nil))
}

func (h *history) open(s string) (cursorPayload, bool) {
	var p cursorPayload
	body, mac, ok := strings.Cut(s, ".")
	enc := base64.RawURLEncoding
	b, err1 := enc.DecodeString(body)
	sum, err2 := enc.DecodeString(mac)
	if !ok || err1 != nil || err2 != nil {
		return p, false
	}
	m := hmac.New(sha256.New, h.key)
	m.Write(b)
	if !hmac.Equal(sum, m.Sum(nil)) || json.Unmarshal(b, &p) != nil {
		return p, false
	}
	return p, true
}

func normalizeFilter(f HistoryFilter) (HistoryFilter, *Error) {
	bad := func(m string) (HistoryFilter, *Error) { return f, newErr(CodeInvalidArgument, m) }
	if f.CreatedFromMs < 0 || f.CreatedBeforeMs < 0 || (f.CreatedFromMs > 0 && f.CreatedBeforeMs > 0 && f.CreatedFromMs >= f.CreatedBeforeMs) {
		return bad("invalid created time range")
	}
	for _, k := range f.Kinds {
		if k != HistoryText && k != HistoryImage && k != HistoryFiles {
			return bad("unknown history kind")
		}
	}
	for _, k := range f.OriginKinds {
		if k != OriginKindLocalSystem && k != OriginKindRemoteDevice && k != OriginKindPlugin && k != OriginKindExplicit {
			return bad("unknown origin kind")
		}
	}
	tags, err := normalizeTags(f.Tags)
	if err != nil {
		return f, err
	}
	f.Tags = tags
	f.Kinds = slices.Compact(slices.Sorted(slices.Values(f.Kinds)))
	f.OriginKinds = slices.Compact(slices.Sorted(slices.Values(f.OriginKinds)))
	return f, nil
}

func filterKey(f HistoryFilter) string {
	b, _ := json.Marshal(f)
	s := sha256.Sum256(b)
	return base64.RawURLEncoding.EncodeToString(s[:12])
}

func (f HistoryFilter) toStore() store.HistoryFilter {
	out := store.HistoryFilter{Tags: f.Tags, DeviceID: string(f.DeviceID), FromMs: f.CreatedFromMs, BeforeMs: f.CreatedBeforeMs}
	for _, k := range f.Kinds {
		out.Kinds = append(out.Kinds, string(k))
	}
	for _, k := range f.OriginKinds {
		out.OriginKinds = append(out.OriginKinds, string(k))
	}
	return out
}

func (e *Engine) ListHistory(ctx context.Context, req ListHistoryRequest) (HistoryPage, error) {
	release, gerr := e.historyGate(ctx, false, false)
	if gerr != nil {
		return HistoryPage{}, gerr
	}
	defer release()
	limit := req.Limit
	if limit == 0 {
		limit = historyDefaultLim
	}
	if limit < 1 || limit > historyMaxLim {
		return HistoryPage{}, newErr(CodeInvalidArgument, "Limit must be between 1 and 200")
	}
	f, ferr := normalizeFilter(req.Filter)
	if ferr != nil {
		return HistoryPage{}, ferr
	}
	fk := filterKey(f)
	q := store.HistoryQuery{Filter: f.toStore(), Limit: limit + 1, NowMs: time.Now().UnixMilli()}
	if req.Cursor != "" {
		cp, ok := e.hist.open(req.Cursor)
		switch {
		case !ok:
			return HistoryPage{}, newErr(CodeInvalidArgument, "malformed cursor")
		case cp.Inst != e.instance:
			return HistoryPage{}, newErr(CodeResourceExpired, "the cursor belongs to an earlier instance")
		case cp.Filter != fk:
			return HistoryPage{}, newErr(CodeConflict, "the filter differs from the one the cursor was made with")
		}
		q.Upper, q.After = cp.Upper, cp.After
	}
	items, rev, newest, err := e.st.ListHistory(q)
	if err != nil {
		return HistoryPage{}, historyStoreErr(err)
	}
	if q.Upper == 0 {
		q.Upper = newest // the first page fixes the upper bound
	}
	page := HistoryPage{HistoryRevision: rev, Items: []HistoryItem{}}
	if len(items) > limit {
		items = items[:limit]
		page.NextCursor = e.hist.seal(cursorPayload{Inst: e.instance, Filter: fk, Upper: q.Upper, After: items[limit-1].Seq})
	}
	for _, it := range items {
		page.Items = append(page.Items, toHistoryItem(it))
	}
	return page, nil
}

// RetainHistoryContent opens a record's content for reading and returns a
// lease on it. The content keeps its blobs alive until released, even if the
// record is deleted meanwhile.
func (e *Engine) RetainHistoryContent(ctx context.Context, id HistoryID) (ContentHandle, error) {
	release, gerr := e.historyGate(ctx, false, true)
	if gerr != nil {
		return ContentHandle{}, gerr
	}
	defer release()
	h := e.hist
	h.mu.Lock()
	defer h.mu.Unlock()
	it, err := e.st.GetHistory(string(id), time.Now().UnixMilli(), true)
	if err != nil {
		return ContentHandle{}, historyStoreErr(err)
	}
	c := &contentObj{id: ContentID(e.newID()), total: it.TotalBytes, refs: 1, hist: h.blob}
	n := 0
	for _, sp := range it.Parts {
		n++
		p := &partRec{desc: PartDescriptor{ID: PartID("p" + strconv.Itoa(n)), Kind: PartRepresentation, MIME: sp.MIME, Size: sp.Size}}
		switch {
		case sp.Kind == "file" && sp.Ref != "":
			p.desc.Kind, p.desc.Name, p.desc.Referenced = PartFile, sp.Name, true
			p.meta = FileMeta{Size: sp.Size, Version: sp.Version}
			if e.opts.ResolveFileRef != nil {
				p.src = e.opts.ResolveFileRef(sp.Ref)
			}
			if p.src == nil {
				p.src = missingSource{}
			}
		case sp.Kind == "file":
			p.desc.Kind, p.desc.Name = PartFile, sp.Name
			p.path = filepath.Join(h.dir, sp.Blob, sp.Name)
			c.blobs = append(c.blobs, sp.Blob)
		case sp.Blob != "":
			data, rerr := readBlob(filepath.Join(h.dir, sp.Blob, "data"), sp.Size)
			if rerr != nil {
				return ContentHandle{}, rerr
			}
			p.data = data
		default:
			p.data = sp.Inline
		}
		c.parts = append(c.parts, p)
	}
	h.blob.hold(c.blobs)
	cs := e.cs
	cs.mu.Lock()
	lease := LeaseID(e.newID())
	cs.contents[c.id] = c
	cs.leases[lease] = &leaseRec{c: c}
	out := ContentHandle{Content: cs.descriptor(c), LeaseID: lease}
	cs.mu.Unlock()
	return out, nil
}

func readBlob(path string, size int64) ([]byte, *Error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, newErr(CodeResourceExpired, "the stored content is gone")
	}
	defer func() { _ = f.Close() }()
	buf := make([]byte, size)
	if _, err := io.ReadFull(f, buf); err != nil {
		return nil, newErr(CodeResourceExpired, "the stored content is damaged")
	}
	return buf, nil
}

// removeHistory deletes a record without a revision check (rollback, expiry,
// eviction). Its blobs go once nothing shares or reads them.
func (e *Engine) removeHistory(id HistoryID) {
	h := e.hist
	h.mu.Lock()
	defer h.mu.Unlock()
	if blobs, _, err := e.st.DeleteHistory(string(id), 0); err == nil {
		h.dropUnreferenced(e.st, blobs)
	}
}

// missingSource stands in for a referenced file that cannot be resolved.
type missingSource struct{}

func (missingSource) Open(context.Context) (io.ReadCloser, FileMeta, error) {
	return nil, FileMeta{}, fs.ErrNotExist
}
