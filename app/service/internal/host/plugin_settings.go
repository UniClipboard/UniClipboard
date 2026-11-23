package host

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"slices"
	"strings"

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
	return n.pluginsLocked(), nil
}

func (n *Node) pluginsLocked() []api.PluginInfo {
	out := []api.PluginInfo{}
	if n.luaDir == "" {
		return out
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
		needsSettings := false
		if wanted && !active && s.Schema != nil {
			_, err := s.RuntimeConfig(n.luaDir, nil)
			needsSettings = luascripts.MissingRequiredSettings(err)
		}
		restart := !needsSettings && (wanted != active || active && !running.Same(s))
		if !restart && active && s.Manifest.Kind == "go" && s.Schema != nil {
			revision, _, err := s.ReadSettings(n.luaDir)
			restart = err != nil || revision != n.pluginRevisions[id]
		}
		source := s.SourceKind
		if source == "" {
			source = "local"
		}
		var actions []api.PluginActionInfo
		for _, a := range s.Manifest.Actions {
			actions = append(actions, api.PluginActionInfo{
				ID:          a.ID,
				Key:         id + "/" + a.ID,
				Title:       a.Title,
				Accepts:     slices.Clone(a.Accepts),
				Placements:  slices.Clone(a.Placements),
				PostActions: slices.Clone(a.PostActions),
				TimeoutMS:   a.TimeoutMS,
			})
		}
		out = append(out, api.PluginInfo{
			ID:               id,
			ImplementationID: s.Manifest.ID,
			Name:             s.Manifest.Name,
			Summary:          s.Manifest.Summary,
			I18N:             pluginTexts(s.Manifest.I18N),
			Version:          s.Manifest.Version,
			Kind:             s.Manifest.Kind,
			Source:           source,
			Repository:       s.Repository,
			Commit:           s.Commit,
			HasPrevious:      s.HasPrevious,
			PreviousVersion:  s.PreviousVersion,
			Events:           slices.Clone(s.Manifest.Events),
			Actions:          actions,
			Capabilities:     slices.Clone(s.Manifest.Capabilities),
			TransportKind:    s.Manifest.TransportKind,
			ExternalChannels: s.Manifest.ExternalChannels,
			Configured:       wanted,
			RunningVersion:   running.Manifest.Version,
			HasSettings:      s.Schema != nil,
			NeedsSettings:    needsSettings,
			Restart:          restart,
		})
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
	return out
}

func (n *Node) PluginMarketplace(ctx context.Context, refresh bool) (api.PluginMarketplace, error) {
	return luascripts.FetchMarketplace(ctx, n.luaDir, refresh), nil
}

func (n *Node) InstallPlugin(ctx context.Context, in api.PluginInstallRequest) (api.PluginInfo, error) {
	if n.luaDir == "" {
		return api.PluginInfo{}, &api.Error{Status: 400, Reason: "unsupported", Message: "plugin installation unavailable"}
	}
	repo := strings.TrimSpace(in.Repository)
	if repo == "" {
		return api.PluginInfo{}, &api.Error{Status: 400, Reason: "invalid_argument", Message: "repository URL is required"}
	}
	id, err := luascripts.Install(ctx, n.luaDir, luascripts.InstallOptions{
		Repository: repo,
		Ref:        strings.TrimSpace(in.Ref),
		Path:       strings.TrimSpace(in.Path),
		Build:      in.Build,
		Replace:    in.Replace,
	})
	if err != nil {
		return api.PluginInfo{}, &api.Error{Status: 400, Reason: "install_failed", Message: err.Error()}
	}
	n.pluginMu.Lock()
	defer n.pluginMu.Unlock()
	if in.Enable {
		if err := n.setPluginEnabledLocked(id, true); err != nil {
			return api.PluginInfo{}, err
		}
	}
	n.hub.emit(api.Event{Kind: "config"})
	for _, item := range n.pluginsLocked() {
		if item.ID == id {
			return item, nil
		}
	}
	return api.PluginInfo{ID: id, ImplementationID: id}, nil
}

func (n *Node) UninstallPlugin(_ context.Context, id string) error {
	if n.luaDir == "" || !luascripts.ValidID(id) {
		return &api.Error{Status: 400, Reason: "invalid_argument", Message: "invalid plugin id"}
	}
	n.pluginMu.Lock()
	defer n.pluginMu.Unlock()
	err := luascripts.Uninstall(n.luaDir, id, func() error {
		return n.setPluginEnabledLocked(id, false)
	})
	if err != nil {
		return &api.Error{Status: 400, Reason: "invalid_argument", Message: err.Error()}
	}
	n.hub.emit(api.Event{Kind: "config"})
	return nil
}

func (n *Node) RollbackPlugin(_ context.Context, id string) (api.PluginInfo, error) {
	if n.luaDir == "" || !luascripts.ValidID(id) {
		return api.PluginInfo{}, &api.Error{Status: 400, Reason: "invalid_argument", Message: "invalid plugin id"}
	}
	n.pluginMu.Lock()
	defer n.pluginMu.Unlock()
	if err := luascripts.Rollback(n.luaDir, id); err != nil {
		return api.PluginInfo{}, &api.Error{Status: 400, Reason: "invalid_argument", Message: err.Error()}
	}
	n.hub.emit(api.Event{Kind: "config"})
	for _, item := range n.pluginsLocked() {
		if item.ID == id {
			return item, nil
		}
	}
	return api.PluginInfo{ID: id, ImplementationID: id}, nil
}

func (n *Node) setPluginEnabledLocked(id string, on bool) error {
	st := n.cfg.State()
	cfg := st.Config
	cfg.Plugins.Lua = slices.DeleteFunc(slices.Clone(cfg.Plugins.Lua), func(x string) bool { return x == id })
	cfg.Plugins.Order = slices.DeleteFunc(slices.Clone(cfg.Plugins.Order), func(x string) bool { return x == id })
	if cfg.Plugins.Instances != nil {
		nextInstances := make(map[string]config.Instance, len(cfg.Plugins.Instances))
		for k, v := range cfg.Plugins.Instances {
			nextInstances[k] = v
		}
		if in, known := nextInstances[id]; known {
			in.Enabled = nil
			if !on {
				disabled := false
				in.Enabled = &disabled
			}
			nextInstances[id] = in
		}
		cfg.Plugins.Instances = nextInstances
	}
	if on {
		cfg.Plugins.Order = append(cfg.Plugins.Order, id)
	}
	var raw bytes.Buffer
	if err := toml.NewEncoder(&raw).Encode(cfg); err != nil {
		return err
	}
	if _, err := n.cfg.Replace(st.Revision, raw.Bytes()); err != nil {
		if errors.Is(err, config.ErrConflict) {
			return &api.Error{Status: 409, Reason: "conflict", Message: "configuration changed; retry"}
		}
		return err
	}
	return nil
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
	out.Restart = active && !canReload && (!running.Same(s) || revision != n.pluginRevisions[id])
	if !active && slices.Contains(n.cfg.Current().ExternalPlugins(), id) {
		_, validationErr := s.Schema.Normalize(values)
		out.Restart = !luascripts.MissingRequiredSettings(validationErr)
	}
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
	return api.PluginInfo{ID: in.ID, ImplementationID: in.ImplementationID, Name: s.Manifest.Name, Summary: s.Manifest.Summary, I18N: pluginTexts(s.Manifest.I18N), Kind: s.Manifest.Kind, Version: s.Manifest.Version, Configured: false, Restart: false, Capabilities: s.Manifest.Capabilities, TransportKind: s.Manifest.TransportKind, ExternalChannels: s.Manifest.ExternalChannels, HasSettings: s.Schema != nil}, nil
}

func pluginTexts(in map[string]luascripts.Text) map[string]api.PluginText {
	if len(in) == 0 {
		return nil
	}
	out := make(map[string]api.PluginText, len(in))
	for tag, text := range in {
		out[tag] = api.PluginText{Name: text.Name, Summary: text.Summary}
	}
	return out
}
