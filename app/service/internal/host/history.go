package host

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"io"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func apiHistory(it engine.HistoryItem, deviceName string) api.HistoryItem {
	out := api.HistoryItem{
		ID: string(it.ID), Revision: it.Revision, Kind: string(it.Kind), Origin: string(it.Origin.Kind),
		DeviceID: string(it.Origin.DeviceID), DeviceName: deviceName, PluginID: string(it.Origin.PluginID),
		CreatedAt: it.CreatedAtMs, UpdatedAt: it.UpdatedAtMs, ExpiresAt: it.ExpiresAtMs,
		Tags: append([]string{}, it.Tags...), TotalBytes: it.TotalBytes, SyncState: string(it.SyncState),
		Parts:    make([]api.HistoryPart, len(it.Parts)),
		Backends: make([]api.HistoryBackendSync, len(it.BackendSyncStates)),
	}
	for i, b := range it.BackendSyncStates {
		out.Backends[i] = api.HistoryBackendSync{BackendID: string(b.BackendID), State: string(b.State), Disposition: string(b.Disposition), ErrorCode: string(b.ErrorCode)}
	}
	for i, p := range it.Parts {
		out.Parts[i] = api.HistoryPart{Kind: string(p.Kind), MIME: p.MIME, Size: p.Size, Available: p.Available}
	}
	return out
}

// names resolves device names once per call; unknown devices stay unnamed.
func (n *Node) names(ctx context.Context) map[engine.DeviceID]string {
	out := map[engine.DeviceID]string{}
	if ds, err := n.e.ListDevices(ctx); err == nil {
		for _, d := range ds {
			out[engine.DeviceID(d.ID)] = d.DisplayName()
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

// folderListLimit bounds one folder listing shown in a preview.
const folderListLimit = 200

func (n *Node) HistoryFolder(ctx context.Context, id string) (api.FolderListing, error) {
	entries, truncated, err := n.e.ListHistoryFolder(ctx, engine.HistoryID(id), folderListLimit)
	if err != nil {
		return api.FolderListing{}, apiErr(err, false)
	}
	out := api.FolderListing{Entries: make([]api.FolderEntry, 0, len(entries)), Truncated: truncated}
	for _, e := range entries {
		out.Entries = append(out.Entries, api.FolderEntry{Name: e.Name, Directory: e.Directory, Size: e.Size})
	}
	return out, nil
}

func (n *Node) UseHistory(ctx context.Context, id string) error {
	_, err := n.RestoreHistory(ctx, id, randomID())
	return err
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

func (n *Node) Actions(ctx context.Context) ([]api.Action, error) {
	list, err := n.e.ListActions(ctx)
	if err != nil {
		return nil, apiErr(err, false)
	}
	out := make([]api.Action, 0, len(list))
	for _, a := range list {
		out = append(out, api.Action{Key: a.Key, PluginID: string(a.PluginID), Title: a.Title, Accepts: a.Accepts})
	}
	return out, nil
}

// RunAction runs an action on a record and puts the result on the clipboard.
// The result is a new record; if the clipboard cannot be written it stays in
// history and can be used from there.
func (n *Node) RunAction(ctx context.Context, id, key string) (api.HistoryItem, error) {
	out, err := n.ExecuteAction(ctx, id, key, api.ActionInput{})
	return out.Item, err
}

func randomID() string {
	b := make([]byte, 12)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

func (n *Node) RestoreHistory(ctx context.Context, id, request string) (engine.RestoreSyncResult, error) {
	out, err := n.clip.RestoreHistory(ctx, engine.HistoryID(id), request)
	return out, apiErr0(err)
}
func (n *Node) FileCacheState(ctx context.Context) (api.FileCacheState, error) {
	out, err := n.e.GetFileCacheState(ctx)
	return api.FileCacheState{FileCacheState: out, CustomDestination: n.customReceive}, apiErr0(err)
}

func (n *Node) ListHistoryTags(ctx context.Context) ([]string, error) {
	tags, err := n.e.ListHistoryTags(ctx)
	return tags, apiErr0(err)
}
