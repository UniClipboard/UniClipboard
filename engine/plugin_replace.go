package engine

import (
	"context"
	"slices"

	"github.com/uniclipboard/unic/internal/pluginproc"
	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin"
)

type externalReplacement struct {
	id                 PluginID
	old, next          plugin.Plugin
	oldBackend         *pluginBackend
	nextBackend        *pluginBackend
	oldSpec            pluginproc.Spec
	oldOptions         pluginproc.Options
	oldConfig          plugin.Config
	newConfig          plugin.Config
	imageFilesAsImages bool
}

// ReplaceExternalPlugins updates active instances of one implementation as a
// group. Both versions must support live start and have the same capability
// class. The previous executables are restarted if preparation or commit fails.
// The caller's installation transaction owns rollback of its persisted pin.
func (e *Engine) ReplaceExternalPlugins(ctx context.Context, instances []PluginInstance, commit func() error) (failure error) {
	e.pluginStartMu.Lock()
	defer e.pluginStartMu.Unlock()
	release, gateErr := e.enter(ctx)
	if gateErr != nil {
		return gateErr
	}
	defer release()
	if len(instances) == 0 || commit == nil {
		return newErr(CodeInvalidArgument, "plugin replacements and an installation commit are required")
	}
	e.mu.Lock()
	ready := e.life == LifecycleRunning && !e.stopReq && !e.closing
	e.mu.Unlock()
	if !ready {
		return newErr(CodeInvalidState, "the engine is not running")
	}

	changes := make([]externalReplacement, 0, len(instances))
	var bindings []store.BackendInstance
	seen := make(map[PluginID]bool, len(instances))
	for _, in := range instances {
		pp := in.Process
		if pp == nil || in.Plugin != nil || in.Builtin || in.Disabled || pp.Dir != "" || !SupportsLivePluginStart(pp.Spec) {
			return newErr(CodeUnsupported, "this plugin cannot be updated live")
		}
		id := PluginID(in.instanceID())
		if seen[id] || pp.Spec.ID != instances[0].Process.Spec.ID {
			return newErr(CodeInvalidArgument, "replacements must name distinct instances of one implementation")
		}
		seen[id] = true
		e.mu.Lock()
		old := e.pluginInstances[id]
		e.mu.Unlock()
		source, ok := old.(interface {
			ReplacementSource() (pluginproc.Spec, pluginproc.Options, plugin.Config)
		})
		if !ok || old.Spec().ID != pp.Spec.ID || !SupportsLivePluginStart(old.Spec()) || old.Spec().ExternalChannels != pp.Spec.ExternalChannels {
			return newErr(CodeConflict, "the running plugin cannot be replaced by this version")
		}
		oldSpec, oldOptions, oldConfig := source.ReplacementSource()
		next := pluginproc.New(pluginproc.Spec{Path: pp.Path, SHA256: pp.SHA256, Args: pp.Args, Env: pp.Env,
			Dir: oldSpec.Dir, Expected: pp.Spec, Events: in.Events}, pp.Options)
		in.Plugin = next
		if err := validateSpec(pp.Spec, in); err != nil {
			return err
		}
		change := externalReplacement{id: id, old: old, next: next, oldSpec: oldSpec,
			oldOptions: oldOptions, oldConfig: oldConfig, newConfig: in.Config, imageFilesAsImages: in.ImageFilesAsImages}
		if pp.Spec.ExternalChannels {
			change.oldBackend = e.channelBackend(id)
			if change.oldBackend == nil {
				return newErr(CodeConflict, "the running external backend is missing")
			}
			binding, err := backendInstanceBinding(string(id), pp.Spec.ID, in.Config)
			if err != nil {
				return err
			}
			bindings = append(bindings, binding)
		}
		changes = append(changes, change)
	}
	if err := ctx.Err(); err != nil {
		return ctxErr(err)
	}

	restoreCtx := context.WithoutCancel(ctx)
	for _, change := range changes {
		if change.oldBackend != nil {
			change.oldBackend.preparing.Store(true)
		}
		if oldEnr := e.findEnricher(change.id); oldEnr != nil {
			oldEnr.cancelAll()
		}
		_ = change.old.Close(restoreCtx)
	}
	defer func() {
		if failure == nil {
			return
		}
		for _, change := range changes {
			_ = change.next.Close(restoreCtx)
		}
		recovered := make([]externalReplacement, 0, len(changes))
		for _, change := range changes {
			previous := pluginproc.New(change.oldSpec, change.oldOptions)
			backend, err := e.startExternalProcess(restoreCtx, change.id, previous, change.oldConfig, change.oldBackend != nil && change.oldBackend.imageFilesAsImages)
			if err != nil {
				_ = previous.Close(restoreCtx)
				failure = newErr(CodeUnavailable, "plugin update failed and the previous process could not restart")
				continue
			}
			change.next, change.nextBackend = previous, backend
			recovered = append(recovered, change)
		}
		e.publishExternalReplacements(recovered)
	}()

	for i := range changes {
		change := &changes[i]
		backend, err := e.startExternalProcess(ctx, change.id, change.next, change.newConfig, change.imageFilesAsImages)
		if err != nil {
			return err
		}
		change.nextBackend = backend
	}
	if err := ctx.Err(); err != nil {
		return ctxErr(err)
	}
	for _, change := range changes {
		if change.next.(interface{ RuntimeStatus() string }).RuntimeStatus() != "running" {
			return newErr(CodeUnavailable, "the replacement plugin stopped during startup")
		}
	}
	if err := commit(); err != nil {
		return err
	}
	if err := e.bindBackendInstances(bindings); err != nil {
		return err
	}
	e.publishExternalReplacements(changes)
	return nil
}

func (e *Engine) publishExternalReplacements(changes []externalReplacement) {
	e.mu.Lock()
	defer e.mu.Unlock()
	for _, change := range changes {
		e.plugins[slices.Index(e.plugins, change.old)] = change.next
		e.pluginInstances[change.id] = change.next
		if change.nextBackend != nil {
			e.syncBackends[slices.Index(e.syncBackends, syncBackend(change.oldBackend))] = change.nextBackend
			change.nextBackend.preparing.Store(false)
		} else {
			e.actions = slices.DeleteFunc(e.actions, func(a *pipelineAction) bool { return a.plugin.id == change.id })
			if a, ok := change.next.(plugin.Actor); ok {
				e.registerActions(change.next.Spec(), a)
			}
			e.enrichers = slices.DeleteFunc(e.enrichers, func(pe *pipelineEnricher) bool { return pe.id == change.id })
			if enr, ok := change.next.(plugin.Enricher); ok {
				e.registerEnricher(change.next.Spec(), enr)
			}
		}
	}
	if len(changes) > 0 {
		e.commitLocked(EventCapabilities, EventDevices)
	}
}
