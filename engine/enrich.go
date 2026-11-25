package engine

import (
	"context"
	"encoding/json"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

const (
	enrichDefaultTimeout = 15 * time.Second
	enrichMaxPriorityQ   = 128
	enrichMaxBgQ         = 256
	enrichGlobalWorkers  = 2
	linkPreviewMaxFavB   = 64 << 10
	linkPreviewMaxThumbB = 256 << 10
)

type pipelineEnricher struct {
	id      PluginID
	version string
	spec    plugin.EnricherSpec
	plugin  plugin.Enricher
	gate    chan struct{}

	mu      sync.Mutex
	active  map[uint64]context.CancelFunc
	nextSeq uint64
}

func newPipelineEnricher(spec plugin.Spec, p plugin.Enricher) *pipelineEnricher {
	es := plugin.EnricherSpec{}
	if spec.Enricher != nil {
		es = plugin.EnricherSpec{
			Accepts:      slices.Clone(spec.Enricher.Accepts),
			RequiredTags: slices.Clone(spec.Enricher.RequiredTags),
			Triggers:     slices.Clone(spec.Enricher.Triggers),
			Annotations:  slices.Clone(spec.Enricher.Annotations),
			TimeoutMS:    spec.Enricher.TimeoutMS,
		}
	}
	return &pipelineEnricher{
		id:      PluginID(spec.ID),
		version: spec.Version,
		spec:    es,
		plugin:  p,
		gate:    make(chan struct{}, 1),
		active:  map[uint64]context.CancelFunc{},
	}
}

func (pe *pipelineEnricher) track(cancel context.CancelFunc) uint64 {
	pe.mu.Lock()
	defer pe.mu.Unlock()
	pe.nextSeq++
	id := pe.nextSeq
	pe.active[id] = cancel
	return id
}

func (pe *pipelineEnricher) untrack(id uint64) {
	pe.mu.Lock()
	delete(pe.active, id)
	pe.mu.Unlock()
}

func (pe *pipelineEnricher) cancelAll() {
	pe.mu.Lock()
	cancels := make([]context.CancelFunc, 0, len(pe.active))
	for _, c := range pe.active {
		cancels = append(cancels, c)
	}
	pe.mu.Unlock()
	for _, c := range cancels {
		c()
	}
}

func (pe *pipelineEnricher) matches(kind string, tags []string, trigger string) bool {
	if !slices.Contains(pe.spec.Triggers, trigger) {
		return false
	}
	if !slices.Contains(pe.spec.Accepts, kind) {
		return false
	}
	for _, req := range pe.spec.RequiredTags {
		if !slices.Contains(tags, req) {
			return false
		}
	}
	return true
}

type enrichKey struct {
	historyID string
	pluginID  PluginID
}

type enrichTask struct {
	historyID string
	pluginID  PluginID
	trigger   string
	force     bool
}

type enrichmentScheduler struct {
	e      *Engine
	mu     sync.Mutex
	notify chan struct{}
	prio   []enrichTask
	bg     []enrichTask
	queued map[enrichKey]bool

	running bool
	ctx     context.Context
	cancel  context.CancelFunc
	sem     chan struct{}
	wg      sync.WaitGroup
}

func newEnrichmentScheduler(e *Engine) *enrichmentScheduler {
	return &enrichmentScheduler{
		e:      e,
		notify: make(chan struct{}, 1),
		queued: map[enrichKey]bool{},
		sem:    make(chan struct{}, enrichGlobalWorkers),
	}
}

func (s *enrichmentScheduler) start(parent context.Context) {
	s.mu.Lock()
	if s.running {
		s.mu.Unlock()
		return
	}
	s.ctx, s.cancel = context.WithCancel(parent)
	s.running = true
	s.wg.Add(1)
	s.mu.Unlock()
	go s.loop()
}

func (s *enrichmentScheduler) stop() {
	s.mu.Lock()
	if !s.running {
		s.mu.Unlock()
		return
	}
	cancel := s.cancel
	s.running = false
	s.prio = nil
	s.bg = nil
	s.queued = map[enrichKey]bool{}
	s.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	s.wg.Wait()
}

func (s *enrichmentScheduler) enqueue(task enrichTask, high bool) {
	s.mu.Lock()
	if !s.running {
		s.mu.Unlock()
		return
	}
	key := enrichKey{historyID: task.historyID, pluginID: task.pluginID}
	if s.queued[key] {
		if high {
			// Promote from bg to prio if already queued.
			for i, t := range s.bg {
				if t.historyID == task.historyID && t.pluginID == task.pluginID {
					s.bg = slices.Delete(s.bg, i, i+1)
					s.prio = append(s.prio, task)
					break
				}
			}
			for i, t := range s.prio {
				if t.historyID == task.historyID && t.pluginID == task.pluginID {
					if task.force {
						s.prio[i].force = true
						s.prio[i].trigger = task.trigger
					}
					break
				}
			}
		}
		s.mu.Unlock()
		return
	}
	if high {
		if len(s.prio) >= enrichMaxPriorityQ {
			dropped := s.prio[0]
			s.prio = s.prio[1:]
			delete(s.queued, enrichKey{historyID: dropped.historyID, pluginID: dropped.pluginID})
		}
		s.prio = append(s.prio, task)
	} else {
		if len(s.bg) >= enrichMaxBgQ {
			dropped := s.bg[0]
			s.bg = s.bg[1:]
			delete(s.queued, enrichKey{historyID: dropped.historyID, pluginID: dropped.pluginID})
		}
		s.bg = append(s.bg, task)
	}
	s.queued[key] = true
	s.mu.Unlock()
	select {
	case s.notify <- struct{}{}:
	default:
	}
}

func (s *enrichmentScheduler) pop() (enrichTask, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.prio) > 0 {
		t := s.prio[0]
		s.prio = s.prio[1:]
		delete(s.queued, enrichKey{historyID: t.historyID, pluginID: t.pluginID})
		return t, true
	}
	if len(s.bg) > 0 {
		t := s.bg[0]
		s.bg = s.bg[1:]
		delete(s.queued, enrichKey{historyID: t.historyID, pluginID: t.pluginID})
		return t, true
	}
	return enrichTask{}, false
}

func (s *enrichmentScheduler) loop() {
	defer s.wg.Done()
	for {
		task, ok := s.pop()
		if !ok {
			select {
			case <-s.ctx.Done():
				return
			case <-s.notify:
				continue
			}
		}
		select {
		case s.sem <- struct{}{}:
		case <-s.ctx.Done():
			return
		}
		s.wg.Add(1)
		go func(t enrichTask) {
			defer func() {
				<-s.sem
				s.wg.Done()
			}()
			s.e.runEnrichment(s.ctx, t)
		}(task)
	}
}

func (e *Engine) enqueueCommitEnrichment(historyID, kind string, tags []string) {
	e.mu.Lock()
	if len(e.enrichers) == 0 {
		e.mu.Unlock()
		return
	}
	var tasks []enrichTask
	for _, pe := range e.enrichers {
		if pe.matches(kind, tags, plugin.EnrichTriggerOnCommit) {
			tasks = append(tasks, enrichTask{
				historyID: historyID,
				pluginID:  pe.id,
				trigger:   plugin.EnrichTriggerOnCommit,
			})
		}
	}
	e.mu.Unlock()
	for _, t := range tasks {
		e.enrich.enqueue(t, false)
	}
}

func (e *Engine) findEnricher(id PluginID) *pipelineEnricher {
	e.mu.Lock()
	defer e.mu.Unlock()
	for _, pe := range e.enrichers {
		if pe.id == id {
			return pe
		}
	}
	return nil
}

func (e *Engine) runEnrichment(ctx context.Context, task enrichTask) {
	pe := e.findEnricher(task.pluginID)
	if pe == nil || ctx.Err() != nil {
		return
	}
	nowMs := historyNow().UnixMilli()
	if !task.force {
		if rec, ok, err := e.st.EnrichmentStatus(task.historyID, string(pe.id)); err == nil && ok {
			if rec.PluginVersion == pe.version && (rec.Status == plugin.EnrichStatusOK || rec.Status == plugin.EnrichStatusSkipped) {
				return
			}
		}
	}
	it, err := e.st.GetHistory(task.historyID, nowMs, false)
	if err != nil {
		return
	}
	if !pe.matches(it.Kind, it.Tags, task.trigger) {
		return
	}
	select {
	case pe.gate <- struct{}{}:
		defer func() { <-pe.gate }()
	case <-ctx.Done():
		return
	}
	// Re-check status after waiting on gate.
	if !task.force {
		if rec, ok, err := e.st.EnrichmentStatus(task.historyID, string(pe.id)); err == nil && ok {
			if rec.PluginVersion == pe.version && (rec.Status == plugin.EnrichStatusOK || rec.Status == plugin.EnrichStatusSkipped) {
				return
			}
		}
	}
	src, lerr := e.loadHistoryContent(HistoryID(task.historyID))
	if lerr != nil {
		return
	}
	item := describePipelineItem(task.historyID, src, pipelineOrigin{
		kind:     it.OriginKind,
		deviceID: DeviceID(it.DeviceID),
		pluginID: it.PluginID,
	})
	defer e.cs.unref(src)

	timeout := enrichDefaultTimeout
	if pe.spec.TimeoutMS > 0 {
		timeout = time.Duration(pe.spec.TimeoutMS) * time.Millisecond
	}
	callCtx, cancel := context.WithTimeout(ctx, timeout)
	trackID := pe.track(cancel)
	defer func() {
		pe.untrack(trackID)
		cancel()
	}()

	handle, herr := e.RetainContent(callCtx, src.id)
	if herr != nil {
		return
	}
	defer func() { _ = e.ReleaseContent(context.WithoutCancel(callCtx), handle.LeaseID) }()

	ec := &enrichContext{
		active:  true,
		trigger: task.trigger,
		item:    item,
		allowed: pe.spec.Annotations,
	}
	for _, p := range handle.Content.Parts {
		ec.parts = append(ec.parts, plugin.ActionPart{MIME: p.MIME, File: p.Kind == PartFile, Name: p.Name, Size: p.Size})
	}
	sourceCtx, stopSource := context.WithCancel(callCtx)
	defer stopSource()
	opened := map[int]bool{}
	liveReaders := 0
	ec.open = func(readerCtx context.Context, index int) (io.ReadCloser, error) {
		ec.mu.Lock()
		defer ec.mu.Unlock()
		if !ec.active || sourceCtx.Err() != nil {
			return nil, newErr(CodeResourceExpired, "the enrichment has ended")
		}
		if index < 0 || index >= len(handle.Content.Parts) || opened[index] {
			return nil, newErr(CodeInvalidArgument, "invalid or reopened enrich part")
		}
		if liveReaders >= 4 {
			return nil, newErr(CodeLimitExceeded, "too many open enrich parts")
		}
		opened[index] = true
		merged, stop := context.WithCancel(sourceCtx)
		stopReader := context.AfterFunc(readerCtx, stop)
		rc, rerr := e.ReadContent(merged, handle.LeaseID, handle.Content.Parts[index].ID)
		if rerr != nil {
			stopReader()
			stop()
			if ec.fault == nil {
				ec.fault = sourceErr(callCtx, rerr)
			}
			return nil, rerr
		}
		liveReaders++
		reader := &actionReader{Reader: rc, ctx: merged, remaining: handle.Content.Parts[index].Size}
		reader.record = func(rerr error) {
			if merged.Err() != nil {
				return
			}
			ec.mu.Lock()
			defer ec.mu.Unlock()
			if ec.fault == nil {
				ec.fault = sourceErr(callCtx, rerr)
			}
		}
		reader.close = func() error {
			stopReader()
			stop()
			cerr := rc.Close()
			ec.mu.Lock()
			liveReaders--
			ec.mu.Unlock()
			return cerr
		}
		context.AfterFunc(merged, func() { _ = reader.Close() })
		return reader, nil
	}

	started := time.Now()
	callErr := invokeEnricher(callCtx, pe, ec)
	stopSource()
	ec.mu.Lock()
	ec.active = false
	res, set, fault := ec.res, ec.set, ec.fault
	ec.mu.Unlock()

	if ctx.Err() != nil {
		// Engine stopping/suspending or plugin being replaced; do not record failure.
		return
	}
	if fault != nil {
		callErr = fault
	} else if callErr == nil && !set {
		callErr = newErr(CodeInvalidArgument, "enricher produced no result")
	}

	outcome, code := diagnosticOutcome(callErr)
	e.diagnostic(ctx, DiagnosticRecord{
		Stage:       "enrich",
		Result:      outcome,
		Backend:     "none",
		ContentKind: diagnosticKind(src),
		Code:        code,
		Duration:    time.Since(started),
	})

	finishNow := historyNow().UnixMilli()
	if callErr != nil {
		e.hist.mu.Lock()
		_, _, _, _, _ = e.st.SaveEnrichment(store.SaveEnrichmentInput{
			ItemID:        task.historyID,
			PluginID:      string(pe.id),
			PluginVersion: pe.version,
			Status:        "failed",
			NowMs:         finishNow,
		})
		e.hist.mu.Unlock()
		return
	}

	if res.Status == plugin.EnrichStatusSkipped {
		e.hist.mu.Lock()
		_, _, _, _, _ = e.st.SaveEnrichment(store.SaveEnrichmentInput{
			ItemID:        task.historyID,
			PluginID:      string(pe.id),
			PluginVersion: pe.version,
			Status:        plugin.EnrichStatusSkipped,
			NowMs:         finishNow,
		})
		e.hist.mu.Unlock()
		return
	}

	e.persistEnrichmentResult(task.historyID, pe, res, finishNow)
}

func invokeEnricher(ctx context.Context, pe *pipelineEnricher, c *enrichContext) (err error) {
	defer func() {
		if recover() != nil {
			err = newErr(CodeInternal, "enricher panicked")
		}
	}()
	return pe.plugin.Enrich(ctx, enrichView{c})
}

func (e *Engine) persistEnrichmentResult(historyID string, pe *pipelineEnricher, res plugin.EnrichmentResult, nowMs int64) {
	var totalBytes int64
	for _, ann := range res.Annotations {
		totalBytes += int64(len(ann.PayloadJSON))
		for _, a := range ann.Assets {
			totalBytes += int64(len(a.Data))
		}
	}
	h := e.hist
	var writtenBlobs []string
	cleanupWritten := func() {
		for _, b := range writtenBlobs {
			_ = os.RemoveAll(filepath.Join(h.dir, b))
		}
	}

	var storeAnns []store.Annotation
	for _, ann := range res.Annotations {
		sa := store.Annotation{
			PluginID:      string(pe.id),
			Type:          ann.Type,
			PluginVersion: pe.version,
			PayloadJSON:   append([]byte(nil), ann.PayloadJSON...),
			UpdatedMs:     nowMs,
		}
		for _, a := range ann.Assets {
			blobName, err := writeAnnotationBlob(h.dir, a.Data)
			if err != nil {
				cleanupWritten()
				return
			}
			writtenBlobs = append(writtenBlobs, blobName)
			sa.Assets = append(sa.Assets, store.AnnotationAsset{
				Key:      a.Key,
				MIME:     a.MIME,
				Blob:     blobName,
				ByteSize: int64(len(a.Data)),
			})
		}
		storeAnns = append(storeAnns, sa)
	}

	var searchDocs []store.SearchDocument
	for _, doc := range res.SearchDocs {
		searchDocs = append(searchDocs, store.SearchDocument{
			Text:   doc.Text,
			Folded: strings.ToLower(doc.Text),
			Kind:   doc.Kind,
		})
	}
	addTags, terr := normalizeTags(res.AddTags)
	if terr != nil {
		cleanupWritten()
		return
	}

	// Reserve budget for annotation payloads and assets without evicting the target item.
	if totalBytes > 0 {
		e.cs.mu.Lock()
		over := e.cs.stored+h.bytes.Load()+e.cachedBytes()+totalBytes > e.res.MaxStoredBytes ||
			h.bytes.Load()+totalBytes > h.opts.MaxBytes
		e.cs.mu.Unlock()
		if over {
			cleanupWritten()
			return
		}
	}

	searchBudget := min(h.opts.MaxBytes, 2<<30) / 2
	h.mu.Lock()
	_, _, oldBlobs, changed, err := e.st.SaveEnrichment(store.SaveEnrichmentInput{
		ItemID:        historyID,
		PluginID:      string(pe.id),
		PluginVersion: pe.version,
		Status:        plugin.EnrichStatusOK,
		AddTags:       addTags,
		Annotations:   storeAnns,
		SearchDocs:    searchDocs,
		SearchBudget:  searchBudget,
		MaxTags:       historyMaxTags,
		NowMs:         nowMs,
	})
	if err != nil {
		h.mu.Unlock()
		cleanupWritten()
		return
	}
	if len(oldBlobs) > 0 {
		h.dropUnreferenced(e.st, oldBlobs)
	}
	h.refresh(e.st)
	h.mu.Unlock()
	if changed {
		e.historyChanged(historyID)
	}
}

func writeAnnotationBlob(dir string, data []byte) (string, error) {
	name := "ann-" + randHex(16)
	tmp, err := os.CreateTemp(dir, ".ann-*")
	if err != nil {
		return "", err
	}
	tmpName := tmp.Name()
	fail := func(err error) (string, error) {
		_ = tmp.Close()
		_ = os.Remove(tmpName)
		return "", err
	}
	if _, err := tmp.Write(data); err != nil {
		return fail(err)
	}
	if err := tmp.Sync(); err != nil {
		return fail(err)
	}
	if err := tmp.Close(); err != nil {
		_ = os.Remove(tmpName)
		return "", err
	}
	if err := os.Rename(tmpName, filepath.Join(dir, name)); err != nil {
		_ = os.Remove(tmpName)
		return "", err
	}
	return name, nil
}

type enrichContext struct {
	mu      sync.Mutex
	active  bool
	trigger string
	item    pipelineItem
	allowed []string
	parts   []plugin.ActionPart
	open    func(context.Context, int) (io.ReadCloser, error)
	res     plugin.EnrichmentResult
	set     bool
	fault   *Error
}

func (c *enrichContext) Trigger() string { return c.trigger }

func (c *enrichContext) Item() pipelineItem {
	c.mu.Lock()
	defer c.mu.Unlock()
	return clonePipelineItem(c.item)
}

func (c *enrichContext) Parts() []plugin.ActionPart {
	c.mu.Lock()
	defer c.mu.Unlock()
	return slices.Clone(c.parts)
}

func (c *enrichContext) OpenPart(ctx context.Context, index int) (io.ReadCloser, error) {
	c.mu.Lock()
	active, open := c.active, c.open
	c.mu.Unlock()
	if !active || open == nil {
		return nil, newErr(CodeResourceExpired, "the enrichment has ended")
	}
	return open(ctx, index)
}

func (c *enrichContext) SetResult(res plugin.EnrichmentResult) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.active {
		return newErr(CodeResourceExpired, "the enrichment has ended")
	}
	if c.fault != nil {
		return c.fault
	}
	if c.set {
		c.fault = newErr(CodeInvalidArgument, "enricher already produced a result")
		return c.fault
	}
	if verr := validateEnrichmentResult(res, c.allowed); verr != nil {
		c.fault = verr
		return verr
	}
	c.res, c.set = res, true
	return nil
}

type enrichView struct{ c *enrichContext }

func (v enrichView) Trigger() string            { return v.c.Trigger() }
func (v enrichView) Item() plugin.Item          { return publicItem(v.c.Item()) }
func (v enrichView) Parts() []plugin.ActionPart { return v.c.Parts() }
func (v enrichView) OpenPart(ctx context.Context, index int) (io.ReadCloser, error) {
	return v.c.OpenPart(ctx, index)
}
func (v enrichView) SetResult(res plugin.EnrichmentResult) error { return v.c.SetResult(res) }

func validateEnrichmentResult(res plugin.EnrichmentResult, allowedAnnotations []string) *Error {
	switch res.Status {
	case plugin.EnrichStatusSkipped:
		if len(res.SearchDocs) != 0 || len(res.AddTags) != 0 || len(res.Annotations) != 0 {
			return newErr(CodeInvalidArgument, "skipped enrichment must carry no output")
		}
		return nil
	case plugin.EnrichStatusOK:
	default:
		return newErr(CodeInvalidArgument, "invalid enrichment status")
	}
	if len(res.SearchDocs) > rpc.MaxEnrichSearchDocs {
		return newErr(CodeLimitExceeded, "too many enrichment search docs")
	}
	totalSearch := 0
	for _, d := range res.SearchDocs {
		if !rpc.ValidEnrichDocKind(d.Kind) || d.Text == "" || !utf8.ValidString(d.Text) {
			return newErr(CodeInvalidArgument, "invalid enrichment search doc")
		}
		if len(d.Text) > rpc.MaxEnrichSearchDocBytes {
			return newErr(CodeLimitExceeded, "enrichment search doc too large")
		}
		totalSearch += len(d.Text)
		if totalSearch > rpc.MaxEnrichSearchTotalBytes {
			return newErr(CodeLimitExceeded, "enrichment search docs total too large")
		}
	}
	if len(res.AddTags) > rpc.MaxTags {
		return newErr(CodeLimitExceeded, "too many enrichment tags")
	}
	seenTags := map[string]bool{}
	for _, t := range res.AddTags {
		if t == "" || len(t) > rpc.MaxTagBytes || !utf8.ValidString(t) || seenTags[t] {
			return newErr(CodeInvalidArgument, "invalid enrichment tag")
		}
		seenTags[t] = true
	}
	if len(res.Annotations) > rpc.MaxEnrichAnnotations {
		return newErr(CodeLimitExceeded, "too many enrichment annotations")
	}
	seenTypes := map[string]bool{}
	totalAssets := 0
	for _, ann := range res.Annotations {
		if !rpc.ValidAnnotationType(ann.Type) || seenTypes[ann.Type] || !slices.Contains(allowedAnnotations, ann.Type) {
			return newErr(CodeInvalidArgument, "undeclared or duplicate annotation type")
		}
		seenTypes[ann.Type] = true
		if len(ann.PayloadJSON) == 0 || len(ann.PayloadJSON) > rpc.MaxEnrichAnnotationPayloadBytes || !json.Valid(ann.PayloadJSON) {
			return newErr(CodeInvalidArgument, "invalid annotation payload")
		}
		if len(ann.Assets) > rpc.MaxEnrichAssetsPerAnnotation {
			return newErr(CodeLimitExceeded, "too many annotation assets")
		}
		assetSizes := map[string]int{}
		for _, a := range ann.Assets {
			if !rpc.ValidEnrichAssetKey(a.Key) || !rpc.ValidEnrichAssetMIME(a.MIME) {
				return newErr(CodeInvalidArgument, "invalid annotation asset descriptor")
			}
			if _, dup := assetSizes[a.Key]; dup {
				return newErr(CodeInvalidArgument, "duplicate annotation asset key")
			}
			if len(a.Data) == 0 || len(a.Data) > rpc.MaxEnrichAssetBytes {
				return newErr(CodeLimitExceeded, "annotation asset size out of bounds")
			}
			totalAssets += len(a.Data)
			if totalAssets > rpc.MaxEnrichTotalAssetBytes {
				return newErr(CodeLimitExceeded, "aggregate annotation assets too large")
			}
			assetSizes[a.Key] = len(a.Data)
		}
		if verr := validateAnnotationPayload(ann.Type, ann.PayloadJSON, assetSizes); verr != nil {
			return verr
		}
	}
	return nil
}

func validateAnnotationPayload(annType string, raw []byte, assetSizes map[string]int) *Error {
	switch annType {
	case plugin.AnnotationLinkPreviewV1:
		return validateLinkPreviewV1(raw, assetSizes)
	default:
		return nil
	}
}

func validPlainText(s string, maxRunes int) bool {
	if !utf8.ValidString(s) || utf8.RuneCountInString(s) > maxRunes {
		return false
	}
	return !strings.ContainsFunc(s, func(r rune) bool {
		return unicode.IsControl(r) && r != '\n' && r != '\t'
	})
}

func validateLinkPreviewV1(raw []byte, assetSizes map[string]int) *Error {
	dec := json.NewDecoder(strings.NewReader(string(raw)))
	dec.DisallowUnknownFields()
	var lp plugin.LinkPreviewV1
	if err := dec.Decode(&lp); err != nil {
		return newErr(CodeInvalidArgument, "invalid link_preview.v1 payload")
	}
	if err := dec.Decode(new(any)); err != io.EOF {
		return newErr(CodeInvalidArgument, "trailing link_preview.v1 payload")
	}
	if lp.CanonicalURL != "" {
		if len(lp.CanonicalURL) > 2048 || !validPlainText(lp.CanonicalURL, 2048) {
			return newErr(CodeInvalidArgument, "invalid link preview canonical_url")
		}
		u, err := url.Parse(lp.CanonicalURL)
		if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" {
			return newErr(CodeInvalidArgument, "invalid link preview canonical_url scheme")
		}
	}
	if !validPlainText(lp.SiteName, 128) || !validPlainText(lp.Title, 256) || !validPlainText(lp.Description, 512) {
		return newErr(CodeInvalidArgument, "invalid link preview text field")
	}
	if strings.TrimSpace(lp.Title) == "" && strings.TrimSpace(lp.Description) == "" {
		return newErr(CodeInvalidArgument, "link_preview.v1 requires title or description")
	}
	if lp.FaviconAsset != "" {
		sz, ok := assetSizes[lp.FaviconAsset]
		if !ok {
			return newErr(CodeInvalidArgument, "missing favicon asset")
		}
		if sz > linkPreviewMaxFavB {
			return newErr(CodeLimitExceeded, "favicon asset exceeds limit")
		}
	}
	if lp.ImageAsset != "" {
		sz, ok := assetSizes[lp.ImageAsset]
		if !ok {
			return newErr(CodeInvalidArgument, "missing image asset")
		}
		if sz > linkPreviewMaxThumbB {
			return newErr(CodeLimitExceeded, "thumbnail asset exceeds limit")
		}
	}
	return nil
}

// ---- Public Engine annotation & enrichment APIs ----

// GetHistoryAnnotations returns persisted derived annotations for an existing,
// non-expired history record, ordered by configured plugin priority.
func (e *Engine) GetHistoryAnnotations(ctx context.Context, id HistoryID) ([]HistoryAnnotation, error) {
	release, gerr := e.historyGate(ctx, false, true)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	if id == "" {
		return nil, newErr(CodeInvalidArgument, "history ID is required")
	}
	anns, err := e.st.ListAnnotations(string(id), historyNow().UnixMilli())
	if err != nil {
		return nil, historyStoreErr(err)
	}
	e.mu.Lock()
	order := make(map[PluginID]int, len(e.enrichers))
	for i, pe := range e.enrichers {
		order[pe.id] = i
	}
	e.mu.Unlock()
	rank := func(id PluginID) int {
		if idx, ok := order[id]; ok {
			return idx
		}
		return len(order) + 1
	}
	out := make([]HistoryAnnotation, 0, len(anns))
	for _, a := range anns {
		ha := HistoryAnnotation{
			PluginID:      PluginID(a.PluginID),
			Type:          a.Type,
			PluginVersion: a.PluginVersion,
			Payload:       append(json.RawMessage(nil), a.PayloadJSON...),
			UpdatedAtMs:   a.UpdatedMs,
		}
		for _, asset := range a.Assets {
			ha.Assets = append(ha.Assets, HistoryAnnotationAsset{
				Key:  asset.Key,
				MIME: asset.MIME,
				Size: asset.ByteSize,
			})
		}
		out = append(out, ha)
	}
	slices.SortStableFunc(out, func(a, b HistoryAnnotation) int {
		if ra, rb := rank(a.PluginID), rank(b.PluginID); ra != rb {
			return ra - rb
		}
		if a.PluginID != b.PluginID {
			return strings.Compare(string(a.PluginID), string(b.PluginID))
		}
		return strings.Compare(a.Type, b.Type)
	})
	return out, nil
}

type heldBlobFile struct {
	*os.File
	tracker *blobTracker
	blob    string
	once    sync.Once
}

func (f *heldBlobFile) Close() error {
	var err error
	f.once.Do(func() {
		err = f.File.Close()
		f.tracker.release([]string{f.blob})
	})
	return err
}

// OpenHistoryAnnotationAsset streams one persisted annotation binary asset while
// holding a blob reference against concurrent deletion.
func (e *Engine) OpenHistoryAnnotationAsset(ctx context.Context, id HistoryID, pluginID PluginID, annType, assetKey string) (io.ReadCloser, HistoryAnnotationAsset, error) {
	release, gerr := e.historyGate(ctx, false, true)
	if gerr != nil {
		return nil, HistoryAnnotationAsset{}, gerr
	}
	defer release()
	if id == "" || pluginID == "" || !rpc.ValidAnnotationType(annType) || !rpc.ValidEnrichAssetKey(assetKey) {
		return nil, HistoryAnnotationAsset{}, newErr(CodeInvalidArgument, "invalid annotation asset parameters")
	}
	asset, err := e.st.GetAnnotationAsset(string(id), string(pluginID), annType, assetKey, historyNow().UnixMilli())
	if err != nil {
		return nil, HistoryAnnotationAsset{}, historyStoreErr(err)
	}
	if asset.Blob == "" {
		return nil, HistoryAnnotationAsset{}, newErr(CodeNotFound, "annotation asset not found")
	}
	e.hist.blob.hold([]string{asset.Blob})
	f, err := os.Open(e.hist.blob.path(asset.Blob))
	if err != nil {
		e.hist.blob.release([]string{asset.Blob})
		return nil, HistoryAnnotationAsset{}, newErr(CodeResourceExpired, "annotation asset unavailable")
	}
	meta := HistoryAnnotationAsset{Key: asset.Key, MIME: asset.MIME, Size: asset.ByteSize}
	return &heldBlobFile{File: f, tracker: e.hist.blob, blob: asset.Blob}, meta, nil
}

// EnrichHistory schedules high-priority on-demand or manual enrichment for an
// existing persisted history record.
func (e *Engine) EnrichHistory(ctx context.Context, req HistoryEnrichRequest) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	if req.HistoryID == "" {
		return newErr(CodeInvalidArgument, "history ID is required")
	}
	e.mu.Lock()
	if err := e.needRunningLocked(); err != nil {
		e.mu.Unlock()
		return err
	}
	enrichers := slices.Clone(e.enrichers)
	e.mu.Unlock()

	it, err := e.st.GetHistory(string(req.HistoryID), historyNow().UnixMilli(), false)
	if err != nil {
		return historyStoreErr(err)
	}
	trigger := plugin.EnrichTriggerOnDemand
	if req.Force {
		trigger = plugin.EnrichTriggerManual
	}
	matchedPlugin := false
	for _, pe := range enrichers {
		if req.PluginID != "" && pe.id != req.PluginID {
			continue
		}
		matchedPlugin = true
		if pe.matches(it.Kind, it.Tags, trigger) {
			e.enrich.enqueue(enrichTask{
				historyID: string(req.HistoryID),
				pluginID:  pe.id,
				trigger:   trigger,
				force:     req.Force,
			}, true)
		}
	}
	if req.PluginID != "" && !matchedPlugin {
		return newErr(CodeNotFound, "no such enricher plugin")
	}
	return nil
}
