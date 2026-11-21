package host

import (
	"context"
	"encoding/json"
	"errors"
	"slices"

	"github.com/uniclipboard/unic/app/service/internal/luascripts"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"
)

func (n *Node) Plugins(context.Context) ([]api.PluginInfo, error) {
	n.pluginMu.Lock()
	defer n.pluginMu.Unlock()
	out := []api.PluginInfo{}
	if n.luaDir == "" {
		return out, nil
	}
	found, _ := luascripts.Scan(n.luaDir)
	enabled := n.cfg.Current().ExternalPlugins()
	for id, s := range n.runningPlugins {
		if _, ok := found[id]; !ok {
			found[id] = s
		}
	}
	for id, s := range found {
		running, active := n.runningPlugins[id]
		wanted := slices.Contains(enabled, id)
		restart := wanted != active || active && !running.Same(s)
		if !restart && active && s.Manifest.Kind == "go" && s.Schema != nil {
			revision, _, err := s.ReadSettings(n.luaDir)
			restart = err != nil || revision != n.pluginRevisions[id]
		}
		source := s.SourceKind
		if source == "" {
			source = "local"
		}
		out = append(out, api.PluginInfo{ID: id, Version: s.Manifest.Version, Kind: s.Manifest.Kind, Source: source,
			Capabilities: slices.Clone(s.Manifest.Capabilities), TransportKind: s.Manifest.TransportKind, ExternalChannels: s.Manifest.ExternalChannels, Configured: wanted, RunningVersion: running.Manifest.Version, HasSettings: s.Schema != nil, Restart: restart})
	}
	slices.SortFunc(out, func(a, b api.PluginInfo) int {
		if a.ID < b.ID {
			return -1
		}
		if a.ID > b.ID {
			return 1
		}
		return 0
	})
	return out, nil
}

func (n *Node) PluginSettings(ctx context.Context, id string) (api.PluginSettings, error) {
	return n.pluginSettings(ctx, id, nil)
}

func (n *Node) SavePluginSettings(ctx context.Context, id string, in api.PluginSettingsEdit) (api.PluginSettings, error) {
	return n.pluginSettings(ctx, id, &in)
}

func (n *Node) pluginSettings(ctx context.Context, id string, edit *api.PluginSettingsEdit) (api.PluginSettings, error) {
	n.pluginMu.Lock()
	defer n.pluginMu.Unlock()
	var out api.PluginSettings
	if n.luaDir == "" || !luascripts.ValidID(id) {
		return out, &api.Error{Status: 404, Reason: "not_found", Message: "plugin settings not found"}
	}
	found, _ := luascripts.Scan(n.luaDir)
	s, ok := found[id]
	if !ok || s.Schema == nil {
		return out, &api.Error{Status: 404, Reason: "not_found", Message: "plugin settings not found"}
	}
	revision, values, err := s.ReadSettings(n.luaDir)
	if err != nil {
		return out, err
	}
	running, active := n.runningPlugins[id]
	canReload := active && s.Manifest.Kind == "lua" && running.Same(s)
	if edit != nil {
		if edit.Revision != revision {
			return out, &api.Error{Status: 409, Reason: "conflict", Message: "plugin settings changed"}
		}
		values, err = s.EditSettings(values, edit.Values)
		if err != nil {
			return out, &api.Error{Status: 400, Reason: "invalid_config", Message: err.Error()}
		}
		persist := func() error {
			var err error
			revision, err = s.WriteSettings(n.luaDir, values)
			if err != nil {
				return errors.New("could not save plugin settings")
			}
			return nil
		}
		if canReload {
			var cfg plugin.Config
			if err := json.Unmarshal(values, &cfg); err != nil {
				return out, err
			}
			if err := n.e.ConfigureLuaPlugin(ctx, engine.PluginID(id), cfg, persist); err != nil {
				return out, err
			}
		} else if err := persist(); err != nil {
			return out, err
		}
		n.hub.emit(api.Event{Kind: "config"})
	}
	out, err = s.SettingsView(revision, values)
	out.Restart = active && !canReload && (!running.Same(s) || revision != n.pluginRevisions[id]) ||
		!active && slices.Contains(n.cfg.Current().ExternalPlugins(), id)
	return out, err
}
