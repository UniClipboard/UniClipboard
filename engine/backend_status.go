package engine

import (
	"context"
	"slices"

	"github.com/uniclipboard/unic/plugin"
)

// SyncBackendInfo describes a local runtime without secret transport addresses.
// State reports process/lifecycle availability, not remote connectivity.
type SyncBackendInfo struct {
	ID               PluginID      `json:"id"`
	ImplementationID string        `json:"implementation_id"`
	TransportKind    TransportKind `json:"transport_kind"`
	Pairing          bool          `json:"pairing"`
	ExternalChannels bool          `json:"external_channels"`
	State            string        `json:"state"`
}

func (e *Engine) ListSyncBackends(ctx context.Context) ([]SyncBackendInfo, error) {
	release, err := e.enter(ctx)
	if err != nil {
		return nil, err
	}
	defer release()
	e.mu.Lock()
	life, runtime := e.life, e.runtime
	e.mu.Unlock()
	out := make([]SyncBackendInfo, 0, len(e.syncBackends))
	for _, backend := range e.syncBackends {
		p := e.pluginInstances[backend.ID()]
		if p == nil {
			continue
		}
		spec := p.Spec()
		state := "running"
		if status, ok := p.(interface{ RuntimeStatus() string }); ok {
			state = status.RuntimeStatus()
		}
		if life != LifecycleRunning {
			state = "stopped"
		} else if runtime == RuntimeSuspended {
			state = "suspended"
		}
		_, pairing := p.(plugin.PeerPairing)
		out = append(out, SyncBackendInfo{ID: backend.ID(), ImplementationID: spec.ID, TransportKind: backend.Kind(), Pairing: pairing && !spec.ExternalChannels && spec.TransportKind == plugin.TransportPeerSessions, ExternalChannels: spec.ExternalChannels, State: state})
	}
	slices.SortFunc(out, func(a, b SyncBackendInfo) int {
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
