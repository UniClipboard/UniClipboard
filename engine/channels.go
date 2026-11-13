package engine

import (
	"context"
	"errors"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin"
)

// External channels (decision 0009): peers of a backend that cannot prove a
// unic identity. A channel has no key and no fingerprint; it is authorized
// only by being enabled, and removing it is local unless the backend can
// also leave the remote group.

// Channel is one external channel. It carries no address, credential or
// remote context.
type Channel struct {
	ID           string         `json:"id"`
	BackendID    PluginID       `json:"backend_id"`
	TargetKind   SyncTargetKind `json:"target_kind"`
	Name         string         `json:"name,omitempty"`          // set by the user
	ReportedName string         `json:"reported_name,omitempty"` // what the backend calls it; untrusted text
	Enabled      bool           `json:"enabled"`
	AutoApply    bool           `json:"auto_apply"` // received content is written to the clipboard
	// RemoteRevocation says removing the channel also leaves the remote group.
	// When false, removal is local only and the UI must say so.
	RemoteRevocation bool   `json:"remote_revocation"`
	Revision         int64  `json:"revision"`
	UpdatedMs        int64  `json:"updated_ms"`
	LastActiveMs     int64  `json:"last_active_ms,omitempty"`
	LastError        string `json:"last_error,omitempty"` // a stable class, never raw backend text
}

// DisplayName is the user's name for the channel, else what the backend reports.
func (c Channel) DisplayName() string {
	if c.Name != "" {
		return c.Name
	}
	return c.ReportedName
}

// Peer kinds of the mixed peer list.
const (
	PeerDevice  = "device"
	PeerChannel = "channel"
)

// Peer is one entry of the list users see: a device or an external channel.
type Peer struct {
	Kind    string   `json:"kind"`
	Device  *Device  `json:"device,omitempty"`
	Channel *Channel `json:"channel,omitempty"`
}

// AddChannelRequest enables a channel after the user paired with the remote
// service natively. RemoteContext identifies the group/account (a secret);
// adding the same context twice returns the existing channel.
type AddChannelRequest struct {
	BackendID     PluginID
	RemoteContext []byte
	ReportedName  string
}

type RenameChannelRequest struct {
	ID               string
	ExpectedRevision int64
	Name             string
}

type SetChannelEnabledRequest struct {
	ID               string
	ExpectedRevision int64
	Enabled          bool
}

type SetChannelAutoApplyRequest struct {
	ID               string
	ExpectedRevision int64
	AutoApply        bool
}

type RemoveChannelRequest struct {
	ID               string
	ExpectedRevision int64
}

func channelStoreErr(err error) *Error {
	switch {
	case errors.Is(err, store.ErrChannelNotFound):
		return newErr(CodeNotFound, "no such channel")
	case errors.Is(err, store.ErrChannelConflict):
		return newErr(CodeConflict, "the channel changed; reload it")
	}
	return retryable(newErr(CodeUnavailable, "the channel database failed"))
}

// channelBackend finds an installed external-channel backend.
func (e *Engine) channelBackend(id PluginID) *pluginBackend {
	for _, b := range e.syncBackends {
		if pb, ok := b.(*pluginBackend); ok && pb.id == id && pb.spec.ExternalChannels {
			return pb
		}
	}
	return nil
}

func (e *Engine) toChannel(c store.Channel) Channel {
	out := Channel{ID: c.ID, BackendID: PluginID(c.InstanceID), TargetKind: TargetGroup, Name: c.Name, ReportedName: c.ReportedName,
		Enabled: c.Enabled, AutoApply: c.AutoApply, Revision: c.Revision, UpdatedMs: c.UpdatedMs, LastActiveMs: c.LastActiveMs, LastError: c.LastError}
	if b := e.channelBackend(PluginID(c.InstanceID)); b != nil {
		out.RemoteRevocation = b.spec.RemoteRevocation
	}
	return out
}

func (e *Engine) channelChanged(id string) {
	e.mu.Lock()
	e.commitObjsLocked([]ObjectRef{{Type: "channel", ID: id}}, EventDevices)
	e.mu.Unlock()
}

// enabledChannels are the IDs of the enabled channels of this backend.
func (b *pluginBackend) enabledChannels() []string {
	cs, err := b.e.st.Channels()
	if err != nil {
		return nil
	}
	var out []string
	for _, c := range cs {
		if c.Enabled && PluginID(c.InstanceID) == b.id {
			out = append(out, c.ID)
		}
	}
	return out
}

func (e *Engine) AddChannel(ctx context.Context, req AddChannelRequest) (Channel, error) {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return Channel{}, gerr
	}
	defer release()
	if e.channelBackend(req.BackendID) == nil {
		return Channel{}, newErr(CodeNotFound, "no such external-channel backend")
	}
	if len(req.RemoteContext) == 0 || len(req.RemoteContext) > 256 || !validDeviceName(req.ReportedName) {
		return Channel{}, newErr(CodeInvalidArgument, "invalid remote context or name")
	}
	c, err := e.st.AddChannel(e.newID(), string(req.BackendID), req.RemoteContext, req.ReportedName)
	if err != nil {
		return Channel{}, channelStoreErr(err)
	}
	e.channelChanged(c.ID)
	return e.toChannel(c), nil
}

func (e *Engine) ListChannels(ctx context.Context) ([]Channel, error) {
	release, gerr := e.historyGate(ctx, false, false)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	return e.channelList()
}

func (e *Engine) channelList() ([]Channel, *Error) {
	cs, err := e.st.Channels()
	if err != nil {
		return nil, channelStoreErr(err)
	}
	out := make([]Channel, len(cs))
	for i, c := range cs {
		out[i] = e.toChannel(c)
	}
	return out, nil
}

func (e *Engine) GetChannel(ctx context.Context, id string) (Channel, error) {
	release, gerr := e.historyGate(ctx, false, false)
	if gerr != nil {
		return Channel{}, gerr
	}
	defer release()
	c, err := e.st.Channel(id)
	if err != nil {
		return Channel{}, channelStoreErr(err)
	}
	return e.toChannel(c), nil
}

// ListPeers is the mixed list users see: devices first, then channels, each
// oldest first. Callers that need the difference read Kind.
func (e *Engine) ListPeers(ctx context.Context) ([]Peer, error) {
	release, gerr := e.historyGate(ctx, false, false)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	ds, derr := e.deviceList()
	if derr != nil {
		return nil, derr
	}
	cs, cerr := e.channelList()
	if cerr != nil {
		return nil, cerr
	}
	out := make([]Peer, 0, len(ds)+len(cs))
	for i := range ds {
		out = append(out, Peer{Kind: PeerDevice, Device: &ds[i]})
	}
	for i := range cs {
		out = append(out, Peer{Kind: PeerChannel, Channel: &cs[i]})
	}
	return out, nil
}

func (e *Engine) channelOp(ctx context.Context, id string, rev int64, do func() (store.Channel, error)) (Channel, error) {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return Channel{}, gerr
	}
	defer release()
	if id == "" || rev == 0 {
		return Channel{}, newErr(CodeInvalidArgument, "ID and a non-zero ExpectedRevision are required")
	}
	c, err := do()
	if err != nil {
		return Channel{}, channelStoreErr(err)
	}
	e.channelChanged(c.ID)
	return e.toChannel(c), nil
}

func (e *Engine) RenameChannel(ctx context.Context, req RenameChannelRequest) (Channel, error) {
	if !validDeviceName(req.Name) {
		return Channel{}, newErr(CodeInvalidArgument, "name: at most 64 characters, one line")
	}
	return e.channelOp(ctx, req.ID, req.ExpectedRevision, func() (store.Channel, error) {
		return e.st.SetChannelName(req.ID, req.ExpectedRevision, req.Name)
	})
}

// SetChannelEnabled pauses or resumes the channel. Once it returns, the
// channel is no longer offered to the backend.
func (e *Engine) SetChannelEnabled(ctx context.Context, req SetChannelEnabledRequest) (Channel, error) {
	return e.channelOp(ctx, req.ID, req.ExpectedRevision, func() (store.Channel, error) {
		return e.st.SetChannelEnabled(req.ID, req.ExpectedRevision, req.Enabled)
	})
}

func (e *Engine) SetChannelAutoApply(ctx context.Context, req SetChannelAutoApplyRequest) (Channel, error) {
	return e.channelOp(ctx, req.ID, req.ExpectedRevision, func() (store.Channel, error) {
		return e.st.SetChannelAutoApply(req.ID, req.ExpectedRevision, req.AutoApply)
	})
}

// RemoveChannel removes the channel. The channel is disabled first, so nothing
// more is sent. If the backend can leave the remote group it does so before
// the record is deleted; a failure keeps the record, so a later retry still
// can. Without remote revocation, removal is local only.
func (e *Engine) RemoveChannel(ctx context.Context, req RemoveChannelRequest) error {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return gerr
	}
	defer release()
	if req.ID == "" || req.ExpectedRevision == 0 {
		return newErr(CodeInvalidArgument, "ID and a non-zero ExpectedRevision are required")
	}
	c, err := e.st.Channel(req.ID)
	if err != nil {
		return channelStoreErr(err)
	}
	if c.Revision != req.ExpectedRevision {
		return channelStoreErr(store.ErrChannelConflict)
	}
	if b := e.channelBackend(PluginID(c.InstanceID)); b != nil && b.spec.RemoteRevocation {
		if c.Enabled {
			nc, err := e.st.SetChannelEnabled(c.ID, c.Revision, false) // stop sending before leaving
			if err != nil {
				return channelStoreErr(err)
			}
			c = nc
			e.channelChanged(c.ID)
		}
		if lv, ok := b.sb.(plugin.ChannelLeaver); ok {
			if lerr := lv.LeaveChannel(ctx, c.ID); lerr != nil {
				_ = e.st.ReportChannel(c.ID, c.LastActiveMs, "remove_failed")
				e.channelChanged(c.ID)
				return retryable(newErr(CodeUnavailable, "the backend could not leave the remote group"))
			}
		}
	}
	if err := e.st.RemoveChannel(c.ID, c.Revision); err != nil {
		return channelStoreErr(err)
	}
	e.channelChanged(c.ID)
	return nil
}

// channelAllowed reports whether the channel may deliver content now: it
// exists, belongs to the backend and is enabled.
func (e *Engine) channelAllowed(backend PluginID, id string) (bool, error) {
	c, err := e.st.Channel(id)
	if errors.Is(err, store.ErrChannelNotFound) {
		return false, nil
	} else if err != nil {
		return false, err
	}
	return c.Enabled && PluginID(c.InstanceID) == backend, nil
}

// Receive implements plugin.Receiver for an external-channel backend. The
// content goes through the same admission, received pipeline, history and
// dedup as content from a device, tagged with the channel instead.
func (b *pluginBackend) Receive(ctx context.Context, channelID string, r plugin.Received) error {
	declined := newErr(CodeUnavailable, "the content was declined")
	if ok, err := b.e.channelAllowed(b.id, channelID); err != nil || !ok || len(r.Parts) == 0 || len(r.Parts) > 16 {
		return declined
	}
	req := receiveRequest{sourceApplication: r.SourceApplication, backend: b.id, channel: channelID, source: DeviceID(channelID), origin: channelID, transfer: r.Transfer, view: r.View}
	var snap clip.Snapshot
	for _, p := range r.Parts {
		if p.MIME == "" || len(p.Data) == 0 {
			return declined
		}
		req.total += int64(len(p.Data))
		req.parts = append(req.parts, receivePart{mime: p.MIME, size: int64(len(p.Data))})
		snap.Parts = append(snap.Parts, clip.Part{Format: p.MIME, Data: p.Data})
	}
	adm, ok := b.e.admitReceive(req)
	if !ok {
		return declined
	}
	adm.Done(snap, "", true)
	return nil
}
