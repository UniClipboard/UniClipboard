package host

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"slices"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/app/service/internal/luascripts"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
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
	cfg := n.cfg.Current()
	enabled := cfg.ExternalPlugins()
	for id, in := range cfg.Plugins.Instances {
		if in.Implementation != "" {
			if s, err := luascripts.ResolveInstance(found, in.Implementation, id); err == nil {
				found[id] = s
			}
		}
	}
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
		out = append(out, api.PluginInfo{ID: id, ImplementationID: s.Manifest.ID, Version: s.Manifest.Version, Kind: s.Manifest.Kind, Source: source,
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
	s, err := luascripts.ResolveInstance(found, n.cfg.Current().PluginImplementation(id), id)
	if err != nil || s.Schema == nil {
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

func (n *Node) CreatePluginInstance(ctx context.Context, in api.PluginInstanceCreate) (configState api.PluginInfo, err error) {
	n.pluginMu.Lock()
	defer n.pluginMu.Unlock()
	found, _ := luascripts.Scan(n.luaDir)
	s, err := luascripts.ResolveInstance(found, in.ImplementationID, in.ID)
	if err != nil {
		return configState, &api.Error{Reason: "invalid_argument", Message: err.Error()}
	}
	cfg, err := n.cfg.Current().AddBackendInstance(in.ImplementationID, in.ID)
	if err != nil {
		return configState, &api.Error{Reason: "invalid_argument", Message: err.Error()}
	}
	var raw bytes.Buffer
	if err = toml.NewEncoder(&raw).Encode(cfg); err != nil {
		return configState, err
	}
	if _, err = n.cfg.Replace(in.Revision, raw.Bytes()); err != nil {
		if errors.Is(err, config.ErrConflict) {
			return configState, &api.Error{Status: 409, Reason: "conflict", Message: "configuration changed; read it again"}
		}
		return configState, err
	}
	n.hub.emit(api.Event{Kind: "config"})
	return api.PluginInfo{ID: in.ID, ImplementationID: in.ImplementationID, Kind: s.Manifest.Kind, Version: s.Manifest.Version, Configured: false, Restart: false, Capabilities: s.Manifest.Capabilities, TransportKind: s.Manifest.TransportKind, ExternalChannels: s.Manifest.ExternalChannels, HasSettings: s.Schema != nil}, nil
}
