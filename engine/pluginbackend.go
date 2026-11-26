package engine

import (
	"context"
	"io"
	"slices"
	"sync"
	"sync/atomic"

	"github.com/uniclipboard/unic/plugin"
)

// pluginBackend adapts an in-process plugin.SyncBackend to the engine's
// syncBackend. The plugin never sees engine types, policies or other
// backends; what it reports is mapped to stable states, and its error text is
// dropped because it could carry addresses or tokens.
type pluginBackend struct {
	e                  *Engine
	id                 PluginID
	kind               TransportKind
	sb                 plugin.SyncBackend
	spec               plugin.Spec
	imageFilesAsImages bool
	preparing          atomic.Bool // reject channel callbacks until live publication
}

func (b *pluginBackend) ID() PluginID        { return b.id }
func (b *pluginBackend) Kind() TransportKind { return b.kind }

func (b *pluginBackend) Submit(ctx context.Context, v backendView, h backendHost) (bool, *Error) {
	if b.preparing.Load() {
		return false, newErr(CodeUnavailable, "the backend is changing versions")
	}
	if len(v.devices) > 0 {
		return false, nil // a device selection addresses peer sessions only
	}
	view := plugin.SyncView{SourceApplication: v.set.c.source.Clone(), ID: v.id, TimestampMs: v.tsMs}
	for _, p := range v.set.parts {
		p := p
		view.Parts = append(view.Parts, plugin.SyncPart{
			MIME: p.desc.MIME, File: p.desc.Kind == PartFile, Name: p.desc.Name, Size: p.desc.Size,
			Open: func(ctx context.Context) (io.ReadCloser, error) {
				rc, err := b.e.openPart(ctx, p)
				if err != nil {
					return nil, err
				}
				return rc, nil
			},
		})
	}
	tk := TargetDevice
	switch {
	case b.spec.ExternalChannels:
		tk = TargetGroup
		view.Targets = b.enabledChannels()
		if len(view.Targets) == 0 {
			return false, nil // nobody enabled: nothing to send, nothing to call
		}
	case b.kind == TransportObjectStore:
		tk = TargetRepository
	}
	syncHost := pluginSyncHost{
		b:           b,
		h:           h,
		kind:        tk,
		traceCtx:    v.traceCtx,
		contentKind: diagnosticKind(v.set.c),
		mu:          &sync.Mutex{},
		spans:       map[string]func(TraceSpanFinish){},
	}
	queued, err := b.sb.Submit(ctx, view, syncHost)
	switch {
	case ctx.Err() != nil && err != nil:
		syncHost.finishRemaining("cancelled", CodeCancelled)
		return false, ctxErr(ctx.Err())
	case err != nil:
		syncHost.finishRemaining("failed", CodeUnavailable)
		return false, newErr(CodeUnavailable, "the sync backend failed")
	}
	return queued, nil
}

type pluginSyncHost struct {
	b           *pluginBackend
	h           backendHost
	kind        SyncTargetKind
	traceCtx    context.Context
	contentKind DiagnosticContentKind
	mu          *sync.Mutex
	spans       map[string]func(TraceSpanFinish)
}

func (s pluginSyncHost) key(t string) targetKey { return targetKey{s.b.id, s.kind, t} }

func (s pluginSyncHost) finishRemaining(result DiagnosticResult, code Code) {
	s.mu.Lock()
	remaining := make([]func(TraceSpanFinish), 0, len(s.spans))
	for k, end := range s.spans {
		remaining = append(remaining, end)
		delete(s.spans, k)
	}
	s.mu.Unlock()
	backend := diagnosticBackend(s.b.id)
	for _, end := range remaining {
		end(TraceSpanFinish{Result: result, Code: code, Backend: backend, ContentKind: s.contentKind})
	}
}

func (s pluginSyncHost) Register(targets []string) error {
	if s.b.spec.ExternalChannels {
		allowed := s.b.enabledChannels()
		for _, t := range targets {
			if !slices.Contains(allowed, t) {
				return newErr(CodeInvalidArgument, "target is not an enabled channel of this backend")
			}
		}
	}
	if len(targets) == 0 || len(targets) > 1024 {
		return newErr(CodeInvalidArgument, "a backend registers between 1 and 1024 targets")
	}
	keys := make([]targetKey, len(targets))
	for i, t := range targets {
		if t == "" || len(t) > 128 {
			return newErr(CodeInvalidArgument, "invalid target ID")
		}
		keys[i] = s.key(t)
	}
	if err := s.h.Register(keys); err != nil {
		return err
	}
	backend := diagnosticBackend(s.b.id)
	parent := s.traceCtx
	if parent == nil {
		parent = context.Background()
	}
	for _, t := range targets {
		_, end := s.b.e.startSpan(parent, TraceSpanSpec{
			Name:        "unic.sync.send",
			Kind:        TraceSpanClient,
			Backend:     backend,
			ContentKind: s.contentKind,
		})
		s.mu.Lock()
		if _, exists := s.spans[t]; !exists {
			s.spans[t] = end
		}
		s.mu.Unlock()
	}
	return nil
}

func (s pluginSyncHost) Report(target string, st plugin.SyncState, cause error) {
	var ts TargetState
	var traceResult DiagnosticResult
	var traceCode Code
	terminal := false
	switch st {
	case plugin.SyncSending:
		ts = TargetSending
	case plugin.SyncSent:
		ts = TargetSent
		traceResult, terminal = "succeeded", true
	case plugin.SyncSuperseded:
		ts = TargetSuperseded
		traceResult, terminal = "superseded", true
	case plugin.SyncFailed:
		ts = TargetFailed
		traceResult, traceCode, terminal = "failed", CodeUnavailable, true
	default:
		ts = TargetQueued
	}
	if terminal {
		s.mu.Lock()
		end := s.spans[target]
		delete(s.spans, target)
		s.mu.Unlock()
		if end != nil {
			end(TraceSpanFinish{Result: traceResult, Code: traceCode, Backend: diagnosticBackend(s.b.id), ContentKind: s.contentKind})
		}
	}
	s.h.Report(s.key(target), ts, cause)
}
