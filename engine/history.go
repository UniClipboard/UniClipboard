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
	"sync/atomic"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin"
)

const (
	historyDirName    = "history"
	historyInlineMax  = 256 << 10
	historyDefaultLim = 50
	historyMaxLim     = 200
	historyMaxTags    = 1000
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
	// bytes is what history itself stores. It shares ResourcePolicy.MaxStoredBytes
	// with live content: shared content counts once, referenced files not at all.
	bytes   atomic.Int64
	mu      sync.Mutex // serializes history changes with blob bookkeeping
	opts    HistoryOptions
	cleanup HistoryCleanupSettings
	dir     string
	blob    *blobTracker
	key     []byte // cursor MAC key, persisted so cursors verify across restarts
}

func (o HistoryOptions) withDefaults(maxStored int64) (HistoryOptions, *Error) {
	if o.MaxBytes < 0 {
		return o, newErr(CodeInvalidArgument, "history options must not be negative")
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
	h := &history{opts: opts, dir: dir, key: key,
		blob: &blobTracker{dir: dir, holders: map[string]int{}, doomed: map[string]bool{}}}
	h.refresh(st)
	return h, nil
}

// refresh re-reads how many bytes history stores.
func (h *history) refresh(st *store.Store) {
	if _, b, err := st.HistoryTotals(); err == nil {
		h.bytes.Store(b)
	}
}

// historyNow is the clock for expiry. Expiry is wall-clock by contract: a
// clock set back delays it, one set forward hastens it, and a record that
// was already deleted never comes back.
var historyNow = time.Now

// reserveStored sets n bytes of the shared storage budget aside, making room
// by dropping what can be dropped: received entries nobody is applying (if
// asked to) and then the oldest history items that store bytes of their own.
func (e *Engine) reserveStored(n int64, evictReceived bool) bool {
	for {
		e.cs.mu.Lock()
		if e.cs.stored+e.hist.bytes.Load()+e.cachedBytes()+n <= e.res.MaxStoredBytes {
			e.cs.stored += n
			e.cs.mu.Unlock()
			return true
		}
		e.cs.mu.Unlock()
		if evictReceived && e.evictOldest(true) {
			continue
		}
		if !e.evictOwnedHistory() {
			return false
		}
	}
}

// evictOwnedHistory removes the oldest record that frees storage.
func (e *Engine) evictOwnedHistory() bool {
	h := e.hist
	h.mu.Lock()
	if !h.cleanup.AutoCleanup {
		h.mu.Unlock()
		return false
	}
	candidates, err := e.st.HistoryCleanupCandidates(h.cleanup.ExcludedTags, 0, 1, "owned")
	ok := len(candidates) > 0
	id := ""
	if ok {
		id = candidates[0].ID
	}
	if err != nil || !ok {
		h.mu.Unlock()
		return false
	}
	blobs, _, err := e.st.DeleteHistory(id, 0)
	if err == nil {
		h.dropUnreferenced(e.st, blobs)
		h.refresh(e.st)
		e.reapCachedFiles()
	}
	h.mu.Unlock()
	if err != nil {
		return false
	}
	e.historyChanged(id)
	e.notifyObservers(plugin.EventDeleted, id)
	return true
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
		Origin:      HistoryOrigin{Kind: HistoryOriginKind(it.OriginKind), DeviceID: DeviceID(it.DeviceID), PluginID: PluginID(it.PluginID)},
		SourceApp:   it.SourceApp,
		CreatedAtMs: it.CreatedMs, UpdatedAtMs: it.UpdatedMs, ExpiresAtMs: it.ExpiresMs,
		Tags: append([]string{}, it.Tags...), TotalBytes: it.TotalBytes, SyncState: HistorySyncState(it.SyncState),
		Parts:             make([]HistoryPartSummary, len(it.Parts)),
		BackendSyncStates: []HistoryBackendSyncState{},
	}
	for b, st := range it.BackendStates {
		out.BackendSyncStates = append(out.BackendSyncStates, HistoryBackendSyncState{
			BackendID: PluginID(b), State: HistorySyncState(st.State), Disposition: BackendDisposition(st.Disposition), ErrorCode: Code(st.ErrorCode)})
	}
	sort.Slice(out.BackendSyncStates, func(i, j int) bool { return out.BackendSyncStates[i].BackendID < out.BackendSyncStates[j].BackendID })
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
		if t == "" || !utf8.ValidString(t) || utf8.RuneCountInString(t) > 64 {
			return nil, newErr(CodeInvalidArgument, "a tag must be 1..64 Unicode characters")
		}
		if !seen[t] {
			seen[t] = true
			out = append(out, t)
		}
	}
	if len(out) > historyMaxTags {
		return nil, newErr(CodeInvalidArgument, "at most 1000 tags")
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
	if p.cache != "" {
		return "cache:" + p.cache
	}
	if ps, ok := p.src.(PersistentFileSource); ok {
		return ps.PersistentRef()
	}
	return ""
}

// persistHistory commits a processed item to the history and returns its ID.
// Blobs are written first and the record second, so a failure leaves earlier
// history intact; eviction happens only after the new record exists.
func (e *Engine) persistHistory(ctx context.Context, pc *processedContent, dedup *store.Dedup) (HistoryID, *Error) {
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
	// The record shares the storage budget with live content; the source being
	// committed is replaced by it, so it is not counted twice.
	for {
		e.cs.mu.Lock()
		used := e.cs.stored - c.owned + h.bytes.Load() + e.cachedBytes()
		e.cs.mu.Unlock()
		if used+stored <= e.res.MaxStoredBytes && h.bytes.Load()+stored <= h.opts.MaxBytes {
			break
		}
		if !e.evictOwnedHistory() {
			return "", retryable(newErr(CodeResourceExhausted, "not enough storage for this history item"))
		}
	}
	now := historyNow()
	it := store.HistoryItem{
		SyncBlocked: !pc.policy.Sync,
		ID:          randHex(16), Kind: string(historyKindOf(pc.item.Type)), OriginKind: pc.item.Origin.kind,
		DeviceID: string(pc.item.Origin.deviceID), PluginID: pc.item.Origin.pluginID,
		CreatedMs: now.UnixMilli(), TotalBytes: c.total,
	}
	switch {
	case pc.policy.ExpireAfter > 0:
		it.ExpiresMs = now.Add(pc.policy.ExpireAfter).UnixMilli()
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
	if h.bytes.Load()+stored > h.opts.MaxBytes {
		h.mu.Unlock()
		return fail(newErr(CodeResourceExhausted, "history storage budget exhausted"))
	}
	seq, _, err := e.st.InsertHistoryDedup(it, dedup)
	if err != nil {
		h.mu.Unlock()
		return fail(historyStoreErr(err))
	}
	h.refresh(e.st)
	evicted := e.evictHistoryLocked(seq)
	h.refresh(e.st)
	e.reapCachedFiles()
	h.mu.Unlock()
	e.historyChanged(append([]string{it.ID}, evicted...)...)
	e.notifyObservers(plugin.EventDeleted, evicted...)
	return HistoryID(it.ID), nil
}

// evictHistoryLocked drops the oldest records (never keep) until the limits
// hold. Failures are retried at the next commit. h.mu held.
func (e *Engine) evictHistoryLocked(keep int64) []string {
	return e.cleanupHistoryLocked(keep)
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
	it, err := e.st.GetHistory(string(id), historyNow().UnixMilli(), false)
	if err != nil {
		return HistoryItem{}, historyStoreErr(err)
	}
	return e.historyItem(it), nil
}

type cursorPayload struct {
	Inst      InstanceID `json:"i"`
	Filter    string     `json:"f"`
	Upper     int64      `json:"u"`
	After     int64      `json:"a"`
	AfterTime int64      `json:"t"`
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
		if k != OriginKindLocalSystem && k != OriginKindRemoteDevice && k != OriginKindAction && k != OriginKindExplicit && k != OriginKindExternal && k != OriginKindMigration {
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
	q := store.HistoryQuery{Filter: f.toStore(), Limit: limit + 1, NowMs: historyNow().UnixMilli()}
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
		q.AfterTime = cp.AfterTime
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
		page.NextCursor = e.hist.seal(cursorPayload{Inst: e.instance, Filter: fk, Upper: q.Upper, After: items[limit-1].Seq, AfterTime: items[limit-1].OrderMs})
	}
	for _, it := range items {
		page.Items = append(page.Items, e.historyItem(it))
	}
	return page, nil
}

// loadHistoryContent materializes a record as content with one reference held
// by the caller, who turns it into a lease or releases it.
func (e *Engine) loadHistoryContent(id HistoryID) (*contentObj, *Error) {
	h := e.hist
	h.mu.Lock()
	defer h.mu.Unlock()
	it, err := e.st.GetHistory(string(id), historyNow().UnixMilli(), true)
	if err != nil {
		return nil, historyStoreErr(err)
	}
	c := &contentObj{id: ContentID(e.newID()), total: it.TotalBytes, refs: 1, hist: h.blob}
	n := 0
	for _, sp := range it.Parts {
		n++
		p := &partRec{desc: PartDescriptor{ID: PartID("p" + strconv.Itoa(n)), Kind: PartRepresentation, MIME: sp.MIME, Size: sp.Size}}
		switch {
		case sp.Kind == "file" && strings.HasPrefix(sp.Ref, "cache:"):
			id := strings.TrimPrefix(sp.Ref, "cache:")
			path, ok := e.cache.hold(id)
			if !ok {
				e.cache.release(c.cached)
				return nil, newErr(CodeResourceExpired, "cached file content is unavailable")
			}
			p.desc.Kind, p.desc.Name, p.cache, p.path = PartFile, sp.Name, id, path
			c.cached = append(c.cached, id)
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
				e.cache.release(c.cached)
				return nil, rerr
			}
			p.data = data
		default:
			p.data = sp.Inline
		}
		c.parts = append(c.parts, p)
	}
	h.blob.hold(c.blobs)
	e.cs.mu.Lock()
	e.cs.contents[c.id] = c
	e.cs.mu.Unlock()
	return c, nil
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
	c, lerr := e.loadHistoryContent(id)
	if lerr != nil {
		return ContentHandle{}, lerr
	}
	cs := e.cs
	cs.mu.Lock()
	lease := LeaseID(e.newID())
	cs.leases[lease] = &leaseRec{c: c} // the lease takes over the loading reference
	out := ContentHandle{Content: cs.descriptor(c), LeaseID: lease}
	cs.mu.Unlock()
	return out, nil
}

// ApplyHistoryRequest names a record to write to the system clipboard, with
// the observation position the host last reported (as for BeginApply).
type ApplyHistoryRequest struct {
	// UserRestore distinguishes explicit history restoration from Action application.
	UserRestore      bool
	HistoryID        HistoryID
	ExpectedEpoch    string
	ExpectedSequence uint64
}

// BeginApplyHistory starts writing a stored record to the host clipboard. It
// shares BeginApply's single slot, host-ability and observation checks and the
// ReportApplyResult protocol, but needs neither network nor enabled sync nor
// trust in the original device: the record is already local. It never reruns
// processors, creates history or syncs.
func (e *Engine) BeginApplyHistory(ctx context.Context, req ApplyHistoryRequest) (ApplyTicket, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return ApplyTicket{}, gerr
	}
	defer release()
	if req.HistoryID == "" {
		return ApplyTicket{}, newErr(CodeInvalidArgument, "HistoryID is required")
	}
	c, lerr := e.loadHistoryContent(req.HistoryID)
	if lerr != nil {
		return ApplyTicket{}, lerr
	}
	defer e.cs.unref(c) // the ticket's lease holds its own reference
	e.obsMu.Lock()
	defer e.obsMu.Unlock()
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.life != LifecycleRunning {
		return ApplyTicket{}, newErr(CodeInvalidState, "the engine is not running")
	}
	if e.runtime == RuntimeSuspended {
		return ApplyTicket{}, newErr(CodeSuspended, "the host suspended the engine")
	}
	switch e.host.Apply {
	case AbilityAvailable:
	case AbilityPermissionRequired:
		return ApplyTicket{}, newErr(CodePermissionDenied, "the system denies writing the clipboard")
	case AbilityUnavailable:
		return ApplyTicket{}, retryable(newErr(CodeUnavailable, "the host cannot write the clipboard now"))
	default:
		return ApplyTicket{}, newErr(CodeUnsupported, "the host cannot write the clipboard")
	}
	if e.ticket != nil {
		return ApplyTicket{}, newErr(CodeInUse, "another application is under way")
	}
	if req.ExpectedEpoch != e.obs.epoch || (req.ExpectedEpoch != "" && req.ExpectedSequence != e.obs.seq) {
		return ApplyTicket{}, newErr(CodeConflict, "the clipboard changed since the last observation")
	}
	// Local content: the sync type switches do not apply, only what the host can write.
	if !e.appliable(c, DefaultBackendPolicy()) {
		return ApplyTicket{}, newErr(CodeUnsupported, "nothing in this content can be applied here")
	}
	t := &applyTicket{started: time.Now(), kind: diagnosticKind(c), token: ApplyToken(e.newID()), content: c.id, history: req.HistoryID, userRestore: req.UserRestore}
	t.lease = e.newLease(c)
	e.ticket = t
	e.commitLocked(EventApply)
	return ApplyTicket{Token: t.token, ContentID: t.content, LeaseID: t.lease}, nil
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
	blobs, _, err := e.st.DeleteHistory(string(id), 0)
	if err == nil {
		h.dropUnreferenced(e.st, blobs)
		h.refresh(e.st)
		e.reapCachedFiles()
	}
	h.mu.Unlock()
	if err == nil {
		e.historyChanged(string(id))
	}
}

// missingSource stands in for a referenced file that cannot be resolved.
type missingSource struct{}

func (missingSource) Open(context.Context) (io.ReadCloser, FileMeta, error) {
	return nil, FileMeta{}, fs.ErrNotExist
}

// sweep deletes expired records and returns their IDs.
func (h *history) sweep(st *store.Store) []string {
	h.mu.Lock()
	defer h.mu.Unlock()
	ids, blobs, _, err := st.ExpiredHistory(historyNow().UnixMilli())
	if err != nil || len(ids) == 0 {
		return nil
	}
	h.dropUnreferenced(st, blobs)
	h.refresh(st)
	return ids
}

var historySweepEvery = 30 * time.Second // var so tests can shorten it

// scheduleHistorySweepLocked arms the next expiry scan. e.mu held.
func (e *Engine) scheduleHistorySweepLocked() {
	if e.histTimer != nil {
		e.histTimer.Stop()
	}
	e.histTimer = time.AfterFunc(historySweepEvery, e.sweepTick)
}

func (e *Engine) sweepTick() {
	e.mu.Lock()
	if e.life != LifecycleRunning {
		e.mu.Unlock()
		return
	}
	e.opWG.Add(1) // admitted together with Stop, which waits for it
	e.mu.Unlock()
	defer e.opWG.Done()
	ids := e.hist.sweep(e.st)
	e.reapCachedFiles()
	e.cleanupHistory()
	e.mu.Lock()
	if len(ids) > 0 {
		e.commitObjsLocked(historyRefs(ids), EventHistory)
	}
	if e.life == LifecycleRunning {
		e.scheduleHistorySweepLocked()
	}
	e.mu.Unlock()
	e.notifyObservers(plugin.EventExpired, ids...)
	e.retryCandidates()
}

func historyRefs(ids []string) []ObjectRef {
	refs := make([]ObjectRef, len(ids))
	for i, id := range ids {
		refs[i] = ObjectRef{Type: "history", ID: id}
	}
	return refs
}

// historyChanged announces changed records after their database commit.
func (e *Engine) historyChanged(ids ...string) {
	e.mu.Lock()
	e.commitObjsLocked(historyRefs(ids), EventHistory)
	e.mu.Unlock()
}

// UpdateItem changes tags and/or expiry of a record when ExpectedRevision
// matches. Content is immutable. Setting the values it already has succeeds
// without a new revision or event.
func (e *Engine) UpdateItem(ctx context.Context, req UpdateItemRequest) (HistoryItem, error) {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return HistoryItem{}, gerr
	}
	defer release()
	if req.ID == "" || req.ExpectedRevision == 0 {
		return HistoryItem{}, newErr(CodeInvalidArgument, "ID and a non-zero ExpectedRevision are required")
	}
	if req.Tags == nil && req.ExpiresAtMs == nil {
		return HistoryItem{}, newErr(CodeInvalidArgument, "nothing to update")
	}
	now := historyNow().UnixMilli()
	var tags *[]string
	if req.Tags != nil {
		n, terr := normalizeTags(*req.Tags)
		if terr != nil {
			return HistoryItem{}, terr
		}
		tags = &n
	}
	if x := req.ExpiresAtMs; x != nil && (*x < 0 || (*x > 0 && *x <= now)) {
		return HistoryItem{}, newErr(CodeInvalidArgument, "ExpiresAtMs must be 0 or in the future")
	}
	h := e.hist
	h.mu.Lock()
	it, changed, _, err := e.st.UpdateHistory(string(req.ID), req.ExpectedRevision, tags, req.ExpiresAtMs, now)
	h.mu.Unlock()
	if err != nil {
		return HistoryItem{}, historyStoreErr(err)
	}
	if changed {
		e.historyChanged(string(req.ID))
		e.notifyObservers(plugin.EventUpdated, string(req.ID))
	}
	return toHistoryItem(it), nil
}

// DeleteItem removes a record. Readers, leases and applications that already
// hold its content keep it until they finish; new requests get not_found.
func (e *Engine) DeleteItem(ctx context.Context, req DeleteItemRequest) error {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return gerr
	}
	defer release()
	if req.ID == "" || req.ExpectedRevision == 0 {
		return newErr(CodeInvalidArgument, "ID and a non-zero ExpectedRevision are required")
	}
	h := e.hist
	h.mu.Lock()
	blobs, _, err := e.st.DeleteHistory(string(req.ID), req.ExpectedRevision)
	if err == nil {
		h.dropUnreferenced(e.st, blobs)
		h.refresh(e.st)
		e.reapCachedFiles()
	}
	h.mu.Unlock()
	if err != nil {
		return historyStoreErr(err)
	}
	e.historyChanged(string(req.ID))
	e.notifyObservers(plugin.EventDeleted, string(req.ID))
	return nil
}

func (e *Engine) historyItem(it store.HistoryItem) HistoryItem {
	out := toHistoryItem(it)
	for i, p := range it.Parts {
		out.Parts[i].Available = true
		if strings.HasPrefix(p.Ref, "cache:") {
			out.Parts[i].Available = e.cache != nil && e.cache.available(strings.TrimPrefix(p.Ref, "cache:"))
		}
	}
	return out
}
