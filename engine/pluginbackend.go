package engine

import (
	"context"
	"io"
	"slices"

	"github.com/uniclipboard/unic/plugin"
)

// pluginBackend adapts an in-process plugin.SyncBackend to the engine's
// syncBackend. The plugin never sees engine types, policies or other
// backends; what it reports is mapped to stable states, and its error text is
// dropped because it could carry addresses or tokens.
type pluginBackend struct {
	e    *Engine
	id   PluginID
	kind TransportKind
	sb   plugin.SyncBackend
	spec plugin.Spec
}

func (b *pluginBackend) ID() PluginID        { return b.id }
func (b *pluginBackend) Kind() TransportKind { return b.kind }

func (b *pluginBackend) Submit(ctx context.Context, v backendView, h backendHost) (bool, *Error) {
	view := plugin.SyncView{ID: v.id, TimestampMs: v.tsMs}
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
	queued, err := b.sb.Submit(ctx, view, pluginSyncHost{b: b, h: h, kind: tk})
	switch {
	case ctx.Err() != nil && err != nil:
		return false, ctxErr(ctx.Err())
	case err != nil:
		return false, newErr(CodeUnavailable, "the sync backend failed")
	}
	return queued, nil
}

type pluginSyncHost struct {
	b    *pluginBackend
	h    backendHost
	kind SyncTargetKind
}

func (s pluginSyncHost) key(t string) targetKey { return targetKey{s.b.id, s.kind, t} }

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
	return nil
}

func (s pluginSyncHost) Report(target string, st plugin.SyncState, cause error) {
	var ts TargetState
	switch st {
	case plugin.SyncSending:
		ts = TargetSending
	case plugin.SyncSent:
		ts = TargetSent
	case plugin.SyncSuperseded:
		ts = TargetSuperseded
	case plugin.SyncFailed:
		ts = TargetFailed
	default:
		ts = TargetQueued
	}
	s.h.Report(s.key(target), ts, cause)
}
