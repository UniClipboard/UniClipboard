package host

import (
	"context"
	"slices"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func (n *Node) SyncBackends(ctx context.Context) ([]api.SyncBackendInfo, error) {
	if _, err := n.e.GetState(ctx); err != nil {
		return nil, apiErr(err, false)
	}
	out := append([]api.SyncBackendInfo{}, n.backends...)
	slices.SortFunc(out, func(a, b api.SyncBackendInfo) int {
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
