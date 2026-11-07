package host

import (
	"context"
	"io"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/engine"
)

func apiHistory(it engine.HistoryItem, deviceName string) api.HistoryItem {
	out := api.HistoryItem{
		ID: string(it.ID), Revision: it.Revision, Kind: string(it.Kind), Origin: string(it.Origin.Kind),
		DeviceID: string(it.Origin.DeviceID), DeviceName: deviceName,
		CreatedAt: it.CreatedAtMs, UpdatedAt: it.UpdatedAtMs, ExpiresAt: it.ExpiresAtMs,
		Tags: append([]string{}, it.Tags...), TotalBytes: it.TotalBytes, SyncState: string(it.SyncState),
		Parts: make([]api.HistoryPart, len(it.Parts)),
	}
	for i, p := range it.Parts {
		out.Parts[i] = api.HistoryPart{Kind: string(p.Kind), MIME: p.MIME, Size: p.Size}
	}
	return out
}

// names resolves device names once per call; unknown devices stay unnamed.
func (n *Node) names(ctx context.Context) map[engine.DeviceID]string {
	out := map[engine.DeviceID]string{}
	if ps, err := n.e.ListPeers(ctx); err == nil {
		for _, p := range ps {
			out[p.ID] = p.Name
		}
	}
	return out
}

func (n *Node) ListHistory(ctx context.Context, q api.HistoryQuery) (api.HistoryPage, error) {
	f := engine.HistoryFilter{Tags: q.Tags, DeviceID: engine.DeviceID(q.DeviceID), CreatedFromMs: q.FromMs, CreatedBeforeMs: q.BeforeMs}
	for _, k := range q.Kinds {
		f.Kinds = append(f.Kinds, engine.HistoryKind(k))
	}
	for _, k := range q.Origins {
		f.OriginKinds = append(f.OriginKinds, engine.HistoryOriginKind(k))
	}
	page, err := n.e.ListHistory(ctx, engine.ListHistoryRequest{Limit: q.Limit, Cursor: q.Cursor, Filter: f})
	if err != nil {
		return api.HistoryPage{}, apiErr(err, false)
	}
	names := n.names(ctx)
	out := api.HistoryPage{Items: make([]api.HistoryItem, 0, len(page.Items)), NextCursor: page.NextCursor, Revision: page.HistoryRevision}
	for _, it := range page.Items {
		out.Items = append(out.Items, apiHistory(it, names[it.Origin.DeviceID]))
	}
	return out, nil
}

func (n *Node) GetHistory(ctx context.Context, id string) (api.HistoryItem, error) {
	it, err := n.e.GetItem(ctx, engine.HistoryID(id))
	if err != nil {
		return api.HistoryItem{}, apiErr(err, false)
	}
	return apiHistory(it, n.names(ctx)[it.Origin.DeviceID]), nil
}

// releasing closes the reader and gives the lease back.
type releasing struct {
	io.ReadCloser
	done func()
}

func (r releasing) Close() error { err := r.ReadCloser.Close(); r.done(); return err }

func (n *Node) HistoryContent(ctx context.Context, id string, part int) (api.HistoryContent, error) {
	h, err := n.e.RetainHistoryContent(ctx, engine.HistoryID(id))
	if err != nil {
		return api.HistoryContent{}, apiErr(err, false)
	}
	release := func() { _ = n.e.ReleaseContent(context.Background(), h.LeaseID) }
	if part >= len(h.Content.Parts) {
		release()
		return api.HistoryContent{}, &api.Error{Message: "no such part", Status: 404}
	}
	p := h.Content.Parts[part]
	r, err := n.e.ReadContent(ctx, h.LeaseID, p.ID)
	if err != nil {
		release()
		return api.HistoryContent{}, apiErr(err, false)
	}
	return api.HistoryContent{MIME: p.MIME, Name: p.Name, Size: p.Size, ReadCloser: releasing{r, release}}, nil
}

func (n *Node) UseHistory(ctx context.Context, id string) error {
	return apiErr0(n.clip.UseHistory(ctx, engine.HistoryID(id)))
}

func (n *Node) UpdateHistory(ctx context.Context, id string, u api.HistoryUpdate) (api.HistoryItem, error) {
	it, err := n.e.UpdateItem(ctx, engine.UpdateItemRequest{ID: engine.HistoryID(id), ExpectedRevision: u.Revision, Tags: u.Tags, ExpiresAtMs: u.ExpiresAtMs})
	if err != nil {
		return api.HistoryItem{}, apiErr(err, false)
	}
	return apiHistory(it, n.names(ctx)[it.Origin.DeviceID]), nil
}

func (n *Node) DeleteHistory(ctx context.Context, id string, revision uint64) error {
	return apiErr0(n.e.DeleteItem(ctx, engine.DeleteItemRequest{ID: engine.HistoryID(id), ExpectedRevision: revision}))
}
