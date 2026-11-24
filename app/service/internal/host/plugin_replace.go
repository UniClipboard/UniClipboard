package host

import (
	"context"
	"maps"
	"slices"

	"github.com/uniclipboard/unic/app/service/internal/luascripts"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func (n *Node) replaceInstalledPluginLocked(ctx context.Context, candidate luascripts.Script, commit func() error) error {
	if !canStartLive(candidate) {
		return commit()
	}
	cfg := n.cfg.Current()
	enabled := cfg.ExternalPlugins()
	spec := candidate.Manifest.PluginSpec()
	var replacements []engine.PluginInstance
	revisions := map[string]string{}
	for _, id := range slices.Sorted(maps.Keys(n.runningPlugins)) {
		old := n.runningPlugins[id]
		if old.Manifest.ID != spec.ID {
			continue
		}
		// An implementation is selected once for all of its instances.
		// If even one requires restart, keep the whole running set intact.
		if !canStartLive(old) || old.Manifest.ExternalChannels != spec.ExternalChannels || !slices.Contains(enabled, id) {
			return commit()
		}
		s := candidate.ForInstance(id)
		onError, fallback := cfg.ProcessorSettings(id)
		values, err := s.RuntimeConfig(n.luaDir, fallback)
		if err != nil {
			return &api.Error{Status: 400, Reason: "invalid_config", Message: "the update requires valid plugin settings"}
		}
		revision := ""
		if s.Schema != nil {
			revision, _, err = s.ReadSettings(n.luaDir)
			if err != nil {
				return &api.Error{Status: 400, Reason: "invalid_config", Message: "could not read plugin settings for the update"}
			}
		}
		revisions[id] = revision
		replacements = append(replacements, engine.PluginInstance{
			ID: engine.PluginID(id), Config: values, OnError: onError, Events: s.Manifest.Events,
			Process: &engine.PluginProcess{Path: s.Executable(), SHA256: s.BinarySHA256, Spec: spec},
		})
	}
	if len(replacements) == 0 {
		return commit()
	}
	if err := n.e.ReplaceExternalPlugins(ctx, replacements, commit); err != nil {
		return apiErr0(err)
	}
	n.runningPluginsMu.Lock()
	for id, revision := range revisions {
		n.runningPlugins[id] = candidate.ForInstance(id)
		n.pluginRevisions[id] = revision
	}
	n.runningPluginsMu.Unlock()
	return nil
}
