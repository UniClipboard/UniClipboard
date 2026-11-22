package host

import (
	"context"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func (n *Node) SyncBackends(ctx context.Context) ([]api.SyncBackendInfo, error) {
	backends, err := n.e.ListSyncBackends(ctx)
	if err != nil {
		return nil, apiErr(err, false)
	}
	out := make([]api.SyncBackendInfo, 0, len(backends))
	for _, b := range backends {
		out = append(out, api.SyncBackendInfo{ID: string(b.ID), ImplementationID: b.ImplementationID, TransportKind: string(b.TransportKind), Pairing: b.Pairing, ExternalChannels: b.ExternalChannels, State: b.State})
	}
	return out, nil
}
