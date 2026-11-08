package engine

import (
	"context"
	"errors"
	"slices"

	"github.com/uniclipboard/unic/internal/luaplugin"
	"github.com/uniclipboard/unic/plugin"
)

// LuaPluginSpec declares one Lua plugin. The host supplies the source; the
// engine never reads files. The script's returned table must repeat ID,
// Version, APIVersion and Capabilities exactly. Lua cannot be a sync backend.
type LuaPluginSpec struct {
	ID           string
	Version      string
	APIVersion   int
	Capabilities []string
	Source       string
	Config       plugin.Config
	Disabled     bool
	Events       []string
	OnError      string // "continue" (default) or "block_sync"
}

// luaInstances turns the specs into plugin instances placed after the Go ones.
func luaInstances(specs []LuaPluginSpec) ([]PluginInstance, *Error) {
	var out []PluginInstance
	for _, s := range specs {
		if slices.Contains(s.Capabilities, plugin.CapSyncBackend) {
			return nil, invalidPlugin(s.ID, "Lua cannot be a sync backend")
		}
		p, err := luaplugin.New(luaplugin.Spec{ID: s.ID, Version: s.Version, APIVersion: s.APIVersion, Capabilities: s.Capabilities, Source: s.Source})
		if err != nil {
			code := CodeInvalidArgument
			if errors.Is(err, luaplugin.ErrResource) {
				code = CodeResourceExhausted
			}
			return nil, newErr(code, "Lua plugin "+s.ID+" could not be loaded")
		}
		out = append(out, PluginInstance{Plugin: p, Config: s.Config, Disabled: s.Disabled, Events: s.Events, OnError: s.OnError})
	}
	return out, nil
}

// ReloadLuaPlugin rebuilds a Lua plugin from source (empty: its original
// source) and swaps it in at a callback boundary. If the new script fails to
// load, init or start, the old one keeps running. A plugin that failed a
// callback is unavailable until this succeeds.
func (e *Engine) ReloadLuaPlugin(ctx context.Context, id PluginID, source string) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	i := slices.IndexFunc(e.plugins, func(p plugin.Plugin) bool { return p.Spec().ID == string(id) })
	if i < 0 {
		return newErr(CodeNotFound, "no such plugin")
	}
	r, ok := e.plugins[i].(interface {
		Reload(context.Context, string) error
	})
	if !ok {
		return newErr(CodeUnsupported, "this plugin is not a Lua plugin")
	}
	if err := r.Reload(ctx, source); err != nil {
		switch {
		case ctx.Err() != nil:
			return ctxErr(ctx.Err())
		case errors.Is(err, luaplugin.ErrResource):
			return newErr(CodeResourceExhausted, "the Lua plugin exceeded its limits")
		}
		return newErr(CodeInvalidArgument, "the Lua plugin could not be reloaded")
	}
	return nil
}
