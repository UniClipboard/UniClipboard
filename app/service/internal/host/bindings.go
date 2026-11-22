package host

import (
	"context"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func (n *Node) SetBindingSync(ctx context.Context, id string, in api.BindingEdit) (engine.DeviceBinding, error) {
	out, err := n.e.SetBindingSync(ctx, engine.SetBindingSyncRequest{ID: id, ExpectedRevision: in.Revision, Enabled: in.Enabled})
	return out, apiErr0(err)
}
func (n *Node) RemoveBinding(ctx context.Context, id string, revision int64) error {
	return apiErr0(n.e.RemoveBinding(ctx, engine.BindingRequest{ID: id, ExpectedRevision: revision}))
}
