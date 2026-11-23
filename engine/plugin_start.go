package engine

import (
	"context"
	"errors"
	"slices"
	"strings"

	"github.com/uniclipboard/unic/internal/pluginproc"
	"github.com/uniclipboard/unic/plugin"
)

// SupportsLivePluginStart reports whether an external process can be added
// without rebuilding the Engine's processing chain or peer session registry.
func SupportsLivePluginStart(spec plugin.Spec) bool {
	if len(spec.Capabilities) != 1 {
		return false
	}
	return spec.Capabilities[0] == plugin.CapAction ||
		spec.Capabilities[0] == plugin.CapSyncBackend && spec.ExternalChannels && spec.TransportKind == plugin.TransportObjectStore
}

// StartExternalPlugin adds an inactive external Go Action or external-channel
// backend. The host persists enablement through commit before publication.
func (e *Engine) StartExternalPlugin(ctx context.Context, in PluginInstance, policy BackendSyncPolicy, commit func() error) error {
	e.pluginStartMu.Lock()
	defer e.pluginStartMu.Unlock()
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()

	if in.Process == nil || in.Plugin != nil || in.Builtin || in.Disabled || !SupportsLivePluginStart(in.Process.Spec) || commit == nil {
		return newErr(CodeUnsupported, "this plugin cannot be started live")
	}
	pp := in.Process
	spec := pp.Spec
	id := PluginID(in.instanceID())
	backend := spec.ExternalChannels
	if len(id) > 128 || !pluginIDRe.MatchString(string(id)) || strings.HasPrefix(string(id), "builtin.") || !backend && string(id) != spec.ID {
		return invalidPlugin(string(id), "invalid instance ID")
	}

	e.mu.Lock()
	running := e.life == LifecycleRunning && !e.stopReq && !e.closing
	occupied := e.pluginInstances[id] != nil || slices.ContainsFunc(e.syncBackends, func(b syncBackend) bool { return b.ID() == id })
	full := backend && len(e.syncBackends) >= e.res.MaxSyncBackends
	e.mu.Unlock()
	switch {
	case !running:
		return newErr(CodeInvalidState, "the engine is not running")
	case occupied:
		return newErr(CodeConflict, "the plugin is already active")
	case full:
		return newErr(CodeLimitExceeded, "too many sync backends are enabled")
	}

	dir := pp.Dir
	var lock *dirLock
	var published bool
	if backend {
		if dir != "" {
			return invalidPlugin(spec.ID, "the engine owns a backend's directory")
		}
		var err error
		dir, lock, err = openInstanceDir(e.opts.DataDir, string(id))
		if errors.Is(err, errLocked) {
			return newErr(CodeInUse, "the backend instance is used by another process")
		}
		if err != nil {
			return newErr(CodeUnavailable, "cannot prepare the backend directory")
		}
		defer func() {
			if !published {
				lock.release()
			}
		}()
	}
	in.Plugin = pluginproc.New(pluginproc.Spec{Path: pp.Path, SHA256: pp.SHA256, Args: pp.Args, Env: pp.Env, Dir: dir,
		Expected: spec, Events: in.Events}, pp.Options)
	if err := validateSpec(spec, in); err != nil {
		return err
	}
	defer func() {
		if !published {
			_ = in.Plugin.Close(context.WithoutCancel(ctx))
		}
	}()
	if err := in.Plugin.Init(ctx, &pluginHost{e: e, id: id}, in.Config); err != nil {
		return newErr(CodeUnavailable, "plugin initialization failed")
	}
	var pb *pluginBackend
	if backend {
		sb := in.Plugin.(plugin.SyncBackend)
		pb = &pluginBackend{e: e, id: id, kind: TransportKind(spec.TransportKind), sb: sb, spec: spec}
		pb.preparing.Store(true)
		in.Plugin.(plugin.ReceiverAware).SetReceiver(pb)
	}
	if err := in.Plugin.Start(ctx); err != nil {
		return newErr(CodeUnavailable, "plugin startup failed")
	}
	if err := ctx.Err(); err != nil {
		return ctxErr(err)
	}
	if err := commit(); err != nil {
		return err
	}
	if pb != nil {
		if err := e.bindBackendInstance(string(id), spec.ID, in.Config); err != nil {
			return err
		}
	}

	e.mu.Lock()
	defer e.mu.Unlock()
	if e.life != LifecycleRunning || e.stopReq || e.closing {
		return newErr(CodeInvalidState, "the engine is stopping")
	}
	e.plugins = append(e.plugins, in.Plugin)
	e.pluginInstances[id] = in.Plugin
	if pb == nil {
		e.registerActions(spec, in.Plugin.(plugin.Actor))
		e.commitLocked(EventCapabilities)
	} else {
		e.instLocks = append(e.instLocks, lock)
		e.syncBackends = append(e.syncBackends, pb)
		e.settings.Value.BackendPolicies[id] = policy
		e.settings.Revision++
		pb.preparing.Store(false)
		e.commitLocked(EventSettings, EventCapabilities, EventDevices)
	}
	published = true
	return nil
}
