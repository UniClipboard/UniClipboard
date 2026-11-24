package host

import (
	"context"
	"os"
	"path/filepath"
	"slices"

	"github.com/uniclipboard/unic/app/service/internal/luascripts"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"
)

func canStartLive(s luascripts.Script) bool {
	return s.Commit != "" && s.Manifest.Kind == "go" &&
		engine.SupportsLivePluginStart(s.Manifest.PluginSpec())
}

func (n *Node) StartPlugin(ctx context.Context, id string) (api.PluginInfo, error) {
	n.pluginMu.Lock()
	defer n.pluginMu.Unlock()
	if err := n.startInstalledPluginLocked(ctx, id); err != nil {
		return api.PluginInfo{}, err
	}
	for _, item := range n.pluginsLocked() {
		if item.ID == id {
			return item, nil
		}
	}
	return api.PluginInfo{}, &api.Error{Status: 404, Reason: "not_found", Message: "plugin not found"}
}

func (n *Node) startInstalledPluginLocked(ctx context.Context, id string) error {
	if n.luaDir == "" || !luascripts.ValidID(id) {
		return &api.Error{Status: 404, Reason: "not_found", Message: "plugin not found"}
	}
	cfg := n.cfg.Current()
	found, _ := luascripts.Scan(n.luaDir)
	s, err := luascripts.ResolveInstance(found, cfg.PluginImplementation(id), id)
	if err != nil {
		return &api.Error{Status: 404, Reason: "not_found", Message: "plugin not found"}
	}
	if !canStartLive(s) {
		return &api.Error{Status: 400, Reason: "unsupported", Message: "this plugin cannot be started live"}
	}
	configured := slices.Contains(cfg.ExternalPlugins(), id)
	enable := func() error {
		if configured {
			return nil
		}
		return n.setPluginEnabledLocked(id, true)
	}
	if running, active := n.runningPlugins[id]; active {
		if configured && running.Same(s) {
			return nil
		}
		return &api.Error{Status: 409, Reason: "conflict", Message: "this plugin version is already running"}
	}

	onError, fallback := cfg.ProcessorSettings(id)
	values, err := s.RuntimeConfig(n.luaDir, fallback)
	if luascripts.MissingRequiredSettings(err) {
		if err := enable(); err != nil {
			return err
		}
		n.cfg.RecordPluginStart(id)
		n.hub.emit(api.Event{Kind: "config"})
		return nil
	}
	if err != nil {
		return &api.Error{Status: 400, Reason: "invalid_config", Message: "invalid plugin settings"}
	}
	revision := ""
	if s.Schema != nil {
		revision, _, err = s.ReadSettings(n.luaDir)
		if err != nil {
			return err
		}
	}
	return n.activatePluginLocked(ctx, s, id, onError, values, &revision, enable)
}

func (n *Node) activatePluginLocked(ctx context.Context, s luascripts.Script, id, onError string, values plugin.Config, revision *string, commit func() error) error {
	spec := s.Manifest.PluginSpec()
	process := &engine.PluginProcess{Path: s.Executable(), SHA256: s.BinarySHA256, Spec: spec}
	if !spec.ExternalChannels {
		process.Dir = filepath.Join(n.dataDir, "plugins", "data", id)
		if err := os.MkdirAll(process.Dir, 0700); err != nil {
			return &api.Error{Status: 500, Reason: "unavailable", Message: "could not prepare plugin data directory"}
		}
	}
	in := engine.PluginInstance{ID: engine.PluginID(id), Process: process, Config: values, OnError: onError, Events: s.Manifest.Events, ImageFilesAsImages: s.Manifest.ImageFilesAsImages}
	if err := n.e.StartExternalPlugin(ctx, in, n.cfg.Current().BackendPolicy(id), commit); err != nil {
		if _, ok := err.(*api.Error); ok {
			return err
		}
		return apiErr0(err)
	}
	n.runningPluginsMu.Lock()
	n.runningPlugins[id] = s
	n.runningPluginsMu.Unlock()
	n.pluginRevisions[id] = *revision
	n.cfg.RecordPluginStart(id)
	n.hub.emit(api.Event{Kind: "config"})
	return nil
}
