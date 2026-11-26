package engine

import (
	"context"
	"errors"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// ---- unified devices (docs/architecture/identity-and-trust.md, device contract) ----
//
// A Device is one authenticated peer, listed once however many sync backends
// reach it. Each way in is a DeviceBinding to one backend instance with its own
// authorization and sync switch. Effective permission needs both: a trusted
// device and an authorized binding. Devices enter the list only after a
// backend has carried an identity proof that the engine verified; discovery
// alone never creates a trusted device.

// Device trust values.
const (
	DeviceTrustPending = "pending" // verified, waiting for the user
	DeviceTrustTrusted = "trusted"
)

// Binding states.
const (
	BindingPending    = "pending"
	BindingAuthorized = "authorized"
)

// SyncDirection controls automatic send and receive permission for one content
// category on a device-to-backend binding, from the local device's perspective.
type SyncDirection string

const (
	SyncDirectionBidirectional SyncDirection = "bidirectional"
	SyncDirectionSendOnly      SyncDirection = "send_only"
	SyncDirectionReceiveOnly   SyncDirection = "receive_only"
	SyncDirectionDisabled      SyncDirection = "disabled"
)

// Valid reports whether d is a known SyncDirection value.
func (d SyncDirection) Valid() bool {
	switch d {
	case SyncDirectionBidirectional, SyncDirectionSendOnly, SyncDirectionReceiveOnly, SyncDirectionDisabled:
		return true
	}
	return false
}

// AllowsSend reports whether automatic sends to the peer are permitted.
func (d SyncDirection) AllowsSend() bool {
	return d == SyncDirectionBidirectional || d == SyncDirectionSendOnly
}

// AllowsReceive reports whether receives from the peer are permitted.
func (d SyncDirection) AllowsReceive() bool {
	return d == SyncDirectionBidirectional || d == SyncDirectionReceiveOnly
}

// BindingSyncPolicy configures per-content-type direction on one binding.
// It can only narrow the parent backend instance policy, governs automatic
// sends and receives, and is bypassed by explicit manual sends.
type BindingSyncPolicy struct {
	Text   SyncDirection `json:"text"`
	Images SyncDirection `json:"images"`
	Files  SyncDirection `json:"files"`
}

// DefaultBindingSyncPolicy returns bidirectional policy for all content types.
func DefaultBindingSyncPolicy() BindingSyncPolicy {
	return BindingSyncPolicy{
		Text:   SyncDirectionBidirectional,
		Images: SyncDirectionBidirectional,
		Files:  SyncDirectionBidirectional,
	}
}

// Valid reports whether every field carries a valid SyncDirection.
func (p BindingSyncPolicy) Valid() bool {
	return p.Text.Valid() && p.Images.Valid() && p.Files.Valid()
}

// Normalized replaces empty fields with bidirectional.
func (p BindingSyncPolicy) Normalized() BindingSyncPolicy {
	if p.Text == "" {
		p.Text = SyncDirectionBidirectional
	}
	if p.Images == "" {
		p.Images = SyncDirectionBidirectional
	}
	if p.Files == "" {
		p.Files = SyncDirectionBidirectional
	}
	return p
}

func fromStoreBindingPolicy(p store.BindingPolicy) BindingSyncPolicy {
	p = p.Normalized()
	return BindingSyncPolicy{
		Text:   SyncDirection(p.Text),
		Images: SyncDirection(p.Images),
		Files:  SyncDirection(p.Files),
	}
}

func toStoreBindingPolicy(p BindingSyncPolicy) store.BindingPolicy {
	return store.BindingPolicy{
		Text:   string(p.Text),
		Images: string(p.Images),
		Files:  string(p.Files),
	}
}

func applyBindingSendPolicy(backend BackendSyncPolicy, binding BindingSyncPolicy) BackendSyncPolicy {
	binding = binding.Normalized()
	backend.Text = backend.Text && binding.Text.AllowsSend()
	backend.Images = backend.Images && binding.Images.AllowsSend()
	backend.Files = backend.Files && binding.Files.AllowsSend()
	return backend
}

func applyBindingReceivePolicy(backend BackendSyncPolicy, binding BindingSyncPolicy) BackendSyncPolicy {
	binding = binding.Normalized()
	backend.Text = backend.Text && binding.Text.AllowsReceive()
	backend.Images = backend.Images && binding.Images.AllowsReceive()
	backend.Files = backend.Files && binding.Files.AllowsReceive()
	return backend
}

// Device is one entry of the unified device list. The ID is derived from the
// device's public key and is not the legacy DeviceID of the tailcat session
// layer.
type Device struct {
	ID           string          `json:"id"`
	Name         string          `json:"name,omitempty"`          // set by the user
	ReportedName string          `json:"reported_name,omitempty"` // what the device calls itself; untrusted text
	Fingerprint  string          `json:"fingerprint"`             // to check against the other device's screen
	Trust        string          `json:"trust"`
	Revision     int64           `json:"revision"`
	UpdatedMs    int64           `json:"updated_ms"`
	Bindings     []DeviceBinding `json:"bindings,omitempty"`
}

// DisplayName is the user's name for the device, else what it reports.
func (d Device) DisplayName() string {
	if d.Name != "" {
		return d.Name
	}
	return d.ReportedName
}

// DeviceBinding is one device-to-backend-instance path. It carries no address,
// credential or remote context.
type DeviceBinding struct {
	ID           string            `json:"id"`
	InstanceID   string            `json:"instance_id"`
	State        string            `json:"state"`
	SyncEnabled  bool              `json:"sync_enabled"`
	Policy       BindingSyncPolicy `json:"policy"`
	Revision     int64             `json:"revision"`
	LastActiveMs int64             `json:"last_active_ms,omitempty"`
	LastError    string            `json:"last_error,omitempty"` // a stable class, never raw backend text
	// Connection is the live link on this path right now.
	Connection Connection `json:"connection"`
	// ConnectionPath is the finite transport path classification when known:
	// "direct", "derp", "peer_relay", or "unknown". Never carries raw endpoints.
	ConnectionPath string `json:"connection_path,omitempty"`
	// LatencyMs is the last measured application round-trip time in milliseconds.
	LatencyMs int64 `json:"latency_ms,omitempty"`
	// LastCheckedMs is the Unix millisecond timestamp of the last link check.
	LastCheckedMs int64 `json:"last_checked_ms,omitempty"`
	// Ticket says the device proved it held a pairing ticket for this path.
	// It explains how the request arose; it grants nothing.
	Ticket bool `json:"ticket,omitempty"`
	// RelayRouteReady reports whether this device has a verified E2EE HTTP relay
	// fallback route ready while P2P is disconnected (contains no mailbox/token).
	RelayRouteReady bool `json:"relay_route_ready,omitempty"`
}

// RenameDeviceRequest sets the user's name for a device; "" clears it.
type RenameDeviceRequest struct {
	ID               string
	ExpectedRevision int64
	Name             string
}

// TrustDeviceRequest approves a pending device. The user must have compared
// the fingerprint on both screens, and the request carries the value they saw:
// if the device changed meanwhile it no longer matches.
type TrustDeviceRequest struct {
	ID                  string
	ExpectedRevision    int64
	ExpectedFingerprint string
}

// BindingRequest names a binding at a revision.
type BindingRequest struct {
	ID               string
	ExpectedRevision int64
}

// SetBindingSyncRequest pauses or resumes content sync on one path.
type SetBindingSyncRequest struct {
	ID               string
	ExpectedRevision int64
	Enabled          bool
}

// SetBindingPolicyRequest updates per-type sync directions on one path.
// It affects only subsequent new automatic sends and receives.
type SetBindingPolicyRequest struct {
	ID               string
	ExpectedRevision int64
	Policy           BindingSyncPolicy
}

// UpdateBindingRequest atomically updates sync_enabled and/or policy on one path.
type UpdateBindingRequest struct {
	ID               string
	ExpectedRevision int64
	Enabled          *bool
	Policy           *BindingSyncPolicy
}

// UnpairDeviceRequest ends trust in a device and removes every path to it.
type UnpairDeviceRequest struct {
	ID               string
	ExpectedRevision int64
}

func deviceStoreErr(err error) *Error {
	switch {
	case errors.Is(err, store.ErrDeviceNotFound), errors.Is(err, store.ErrBindingNotFound):
		return newErr(CodeNotFound, "no such device or binding")
	case errors.Is(err, store.ErrDeviceConflict), errors.Is(err, store.ErrBindingConflict):
		return newErr(CodeConflict, "the device changed; reload it")
	case errors.Is(err, store.ErrIdentityConflict):
		return newErr(CodeIdentityMismatch, "the device identity conflicts with a registered one")
	}
	return retryable(newErr(CodeUnavailable, "the device database failed"))
}

func (e *Engine) toDevice(d store.Device, bs []store.Binding, links map[PluginID]map[string]rpc.PeerLinkStatus) Device {
	out := Device{ID: d.ID, Name: d.Name, ReportedName: d.ReportedName, Trust: d.Trust, Revision: d.Revision, UpdatedMs: d.UpdatedMs,
		Fingerprint: identity.Fingerprint(e.ident.Public(), d.PublicKey)}
	for _, b := range bs {
		out.Bindings = append(out.Bindings, e.toBindingWithLinks(b, links))
	}
	return out
}

func (e *Engine) toBinding(b store.Binding) DeviceBinding {
	return e.toBindingWithLinks(b, e.peerLinkSnapshots())
}

func (e *Engine) toBindingWithLinks(b store.Binding, links map[PluginID]map[string]rpc.PeerLinkStatus) DeviceBinding {
	conn := Disconnected
	var path string
	var latencyMs, checkedMs int64
	for _, pb := range e.peerProcs {
		if string(pb.id) != b.InstanceID {
			continue
		}
		var st rpc.PeerLinkStatus
		var hasSt bool
		if snaps := links[pb.id]; snaps != nil {
			st, hasSt = snaps[b.DeviceID]
		}
		if pb.connected(b.DeviceID) {
			conn = Connected
			if hasSt && st.Validate() == nil {
				if st.Path != "" {
					path = st.Path
				}
				latencyMs = st.LatencyMs
				checkedMs = st.LastCheckedMs
			}
		} else if hasSt && st.Validate() == nil {
			if st.State == rpc.LinkStateConnecting && b.State == BindingAuthorized {
				conn = Connecting
			}
			checkedMs = st.LastCheckedMs
		}
	}
	relayReady := b.State == store.BindingAuthorized && b.SyncEnabled && e.relay != nil && e.relay.routeReady(b.DeviceID)
	return DeviceBinding{ID: b.ID, InstanceID: b.InstanceID, State: b.State, SyncEnabled: b.SyncEnabled,
		Policy:   fromStoreBindingPolicy(b.Policy),
		Revision: b.Revision, LastActiveMs: b.LastActiveMs, LastError: b.LastError, Connection: conn,
		ConnectionPath: path, LatencyMs: latencyMs, LastCheckedMs: checkedMs, Ticket: b.Ticket,
		RelayRouteReady: relayReady}
}

func (e *Engine) peerLinkSnapshots() map[PluginID]map[string]rpc.PeerLinkStatus {
	if len(e.peerProcs) == 0 {
		return nil
	}
	out := make(map[PluginID]map[string]rpc.PeerLinkStatus, len(e.peerProcs))
	for _, pb := range e.peerProcs {
		if snaps := pb.linkSnapshots(); len(snaps) > 0 {
			out[pb.id] = snaps
		}
	}
	return out
}

func (e *Engine) deviceChanged(id string) {
	e.mu.Lock()
	e.commitObjsLocked([]ObjectRef{{Type: "device", ID: id}}, EventDevices)
	e.mu.Unlock()
}

// ListDevices returns every device with its paths, oldest first. Unpaired
// devices are not listed.
func (e *Engine) ListDevices(ctx context.Context) ([]Device, error) {
	release, gerr := e.historyGate(ctx, false, false)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	out, derr := e.deviceList()
	if derr != nil {
		return nil, derr
	}
	return out, nil
}

// deviceList is every listed device with its paths, oldest first.
func (e *Engine) deviceList() ([]Device, *Error) {
	ds, err := e.st.Devices()
	if err != nil {
		return nil, deviceStoreErr(err)
	}
	links := e.peerLinkSnapshots()
	out := make([]Device, 0, len(ds))
	for _, d := range ds {
		if d.Trust == store.TrustRevoked {
			continue
		}
		bs, err := e.st.Bindings(d.ID)
		if err != nil {
			return nil, deviceStoreErr(err)
		}
		out = append(out, e.toDevice(d, bs, links))
	}
	return out, nil
}

func (e *Engine) GetDevice(ctx context.Context, id string) (Device, error) {
	release, gerr := e.historyGate(ctx, false, false)
	if gerr != nil {
		return Device{}, gerr
	}
	defer release()
	return e.loadDevice(id)
}

func (e *Engine) loadDevice(id string) (Device, error) {
	d, err := e.st.Device(id)
	if err != nil || d.Trust == store.TrustRevoked {
		if err == nil {
			err = store.ErrDeviceNotFound
		}
		return Device{}, deviceStoreErr(err)
	}
	bs, err := e.st.Bindings(id)
	if err != nil {
		return Device{}, deviceStoreErr(err)
	}
	return e.toDevice(d, bs, e.peerLinkSnapshots()), nil
}

func validDeviceName(s string) bool {
	if len(s) > 64 || !utf8.ValidString(s) || strings.TrimSpace(s) != s {
		return false
	}
	for _, r := range s {
		if unicode.IsControl(r) {
			return false
		}
	}
	return true
}

// RenameDevice sets the user's own name for a device.
func (e *Engine) RenameDevice(ctx context.Context, req RenameDeviceRequest) (Device, error) {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return Device{}, gerr
	}
	defer release()
	if req.ID == "" || req.ExpectedRevision == 0 || !validDeviceName(req.Name) {
		return Device{}, newErr(CodeInvalidArgument, "ID, a non-zero ExpectedRevision and a valid name (at most 64 bytes, no control characters) are required")
	}
	if _, err := e.st.SetDeviceName(req.ID, req.ExpectedRevision, req.Name); err != nil {
		return Device{}, deviceStoreErr(err)
	}
	e.deviceChanged(req.ID)
	return e.loadDevice(req.ID)
}

// TrustDevice approves a pending device after the user compared fingerprints.
// It grants no path: each binding needs its own AuthorizeBinding.
func (e *Engine) TrustDevice(ctx context.Context, req TrustDeviceRequest) (Device, error) {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return Device{}, gerr
	}
	defer release()
	if req.ID == "" || req.ExpectedRevision == 0 || req.ExpectedFingerprint == "" {
		return Device{}, newErr(CodeInvalidArgument, "ID, a non-zero ExpectedRevision and ExpectedFingerprint are required")
	}
	d, err := e.st.Device(req.ID)
	if err != nil {
		return Device{}, deviceStoreErr(err)
	}
	want := identity.Fingerprint(e.ident.Public(), d.PublicKey)
	if normFingerprint(req.ExpectedFingerprint) != normFingerprint(want) {
		return Device{}, newErr(CodeIdentityMismatch, "the fingerprint does not match this device")
	}
	if _, err := e.st.TrustDevice(req.ID, req.ExpectedRevision); err != nil {
		return Device{}, deviceStoreErr(err)
	}
	e.reconcile()
	e.deviceChanged(req.ID)
	return e.loadDevice(req.ID)
}

// ApproveDeviceRequest binds approval to the device and every pending path shown.
type ApproveDeviceRequest struct {
	ID                  string
	ExpectedRevision    int64
	ExpectedFingerprint string
	Bindings            []BindingRequest
}

// ApproveDevice atomically trusts a device and authorizes its reviewed paths.
func (e *Engine) ApproveDevice(ctx context.Context, req ApproveDeviceRequest) (Device, error) {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return Device{}, gerr
	}
	defer release()
	if req.ID == "" || req.ExpectedRevision <= 0 || strings.TrimSpace(req.ExpectedFingerprint) == "" || len(req.Bindings) == 0 {
		return Device{}, newErr(CodeInvalidArgument, "device revision, fingerprint and pending binding revisions are required")
	}
	bindings := make(map[string]int64, len(req.Bindings))
	for _, b := range req.Bindings {
		if b.ID == "" || b.ExpectedRevision <= 0 || bindings[b.ID] != 0 {
			return Device{}, newErr(CodeInvalidArgument, "invalid binding revisions")
		}
		bindings[b.ID] = b.ExpectedRevision
	}
	d, err := e.st.Device(req.ID)
	if err != nil {
		return Device{}, deviceStoreErr(err)
	}
	if normFingerprint(req.ExpectedFingerprint) != normFingerprint(identity.Fingerprint(e.ident.Public(), d.PublicKey)) {
		return Device{}, newErr(CodeIdentityMismatch, "the fingerprint does not match this device")
	}
	if err := e.st.ApproveDevice(req.ID, req.ExpectedRevision, bindings); err != nil {
		return Device{}, deviceStoreErr(err)
	}
	e.reconcile()
	e.deviceChanged(req.ID)
	if d.Trust != store.TrustTrusted {
		e.productEvent(ctx, ProductEvent{Name: "pairing_completed", Result: "succeeded"})
	}
	return e.loadDevice(req.ID)
}

func normFingerprint(s string) string {
	return strings.ToUpper(strings.ReplaceAll(strings.TrimSpace(s), " ", ""))
}

// AuthorizeBinding lets one path of a trusted device carry content.
func (e *Engine) AuthorizeBinding(ctx context.Context, req BindingRequest) (DeviceBinding, error) {
	return e.bindingOp(ctx, req.ID, req.ExpectedRevision, true, func() (store.Binding, error) {
		return e.st.AuthorizeBinding(req.ID, req.ExpectedRevision)
	})
}

// SetBindingSync pauses or resumes content sync on one path only.
func (e *Engine) SetBindingSync(ctx context.Context, req SetBindingSyncRequest) (DeviceBinding, error) {
	return e.bindingOp(ctx, req.ID, req.ExpectedRevision, true, func() (store.Binding, error) {
		return e.st.SetBindingSync(req.ID, req.ExpectedRevision, req.Enabled)
	})
}

// SetBindingPolicy updates per-type automatic sync and receive directions on one
// path. It does not close live sessions or cancel in-flight transfers.
func (e *Engine) SetBindingPolicy(ctx context.Context, req SetBindingPolicyRequest) (DeviceBinding, error) {
	if !req.Policy.Valid() {
		return DeviceBinding{}, newErr(CodeInvalidArgument, "valid text, images and files sync directions are required")
	}
	return e.bindingOp(ctx, req.ID, req.ExpectedRevision, false, func() (store.Binding, error) {
		return e.st.SetBindingPolicy(req.ID, req.ExpectedRevision, toStoreBindingPolicy(req.Policy))
	})
}

// UpdateBinding atomically updates sync_enabled and/or per-type policy by CAS.
// Live sessions are reconciled only when Enabled is modified.
func (e *Engine) UpdateBinding(ctx context.Context, req UpdateBindingRequest) (DeviceBinding, error) {
	if req.Enabled == nil && req.Policy == nil {
		return DeviceBinding{}, newErr(CodeInvalidArgument, "enabled or policy is required")
	}
	var sp *store.BindingPolicy
	if req.Policy != nil {
		if !req.Policy.Valid() {
			return DeviceBinding{}, newErr(CodeInvalidArgument, "valid text, images and files sync directions are required")
		}
		p := toStoreBindingPolicy(*req.Policy)
		sp = &p
	}
	return e.bindingOp(ctx, req.ID, req.ExpectedRevision, req.Enabled != nil, func() (store.Binding, error) {
		return e.st.UpdateBinding(req.ID, req.ExpectedRevision, req.Enabled, sp)
	})
}

func (e *Engine) bindingOp(ctx context.Context, id string, rev int64, reconcile bool, do func() (store.Binding, error)) (DeviceBinding, error) {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return DeviceBinding{}, gerr
	}
	defer release()
	if id == "" || rev == 0 {
		return DeviceBinding{}, newErr(CodeInvalidArgument, "ID and a non-zero ExpectedRevision are required")
	}
	b, err := do()
	if err != nil {
		return DeviceBinding{}, deviceStoreErr(err)
	}
	if reconcile {
		e.reconcile()
		if !b.SyncEnabled && e.relay != nil {
			e.relay.revokePeer(b.DeviceID, false)
		}
	}
	e.deviceChanged(b.DeviceID)
	return e.toBinding(b), nil
}

// RemoveBinding revokes one path; the device and its other paths stay.
func (e *Engine) RemoveBinding(ctx context.Context, req BindingRequest) error {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return gerr
	}
	defer release()
	if req.ID == "" || req.ExpectedRevision == 0 {
		return newErr(CodeInvalidArgument, "ID and a non-zero ExpectedRevision are required")
	}
	b, err := e.st.Binding(req.ID)
	if err != nil {
		return deviceStoreErr(err)
	}
	if err := e.st.RemoveBinding(req.ID, req.ExpectedRevision); err != nil {
		return deviceStoreErr(err)
	}
	if e.relay != nil {
		e.relay.revokePeer(b.DeviceID, true)
	}
	e.reconcile()
	e.deviceChanged(b.DeviceID)
	return nil
}

// UnpairDevice ends trust in the device and removes all its paths. Once the
// database commit returns, no binding of the device authorizes anything.
func (e *Engine) UnpairDevice(ctx context.Context, req UnpairDeviceRequest) error {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return gerr
	}
	defer release()
	if req.ID == "" || req.ExpectedRevision == 0 {
		return newErr(CodeInvalidArgument, "ID and a non-zero ExpectedRevision are required")
	}
	if err := e.st.RevokeDevice(req.ID, req.ExpectedRevision); err != nil {
		return deviceStoreErr(err)
	}
	_ = e.st.DropIntroducer(req.ID) // what a removed device vouched for no longer counts
	if e.relay != nil {
		e.relay.revokePeer(req.ID, true)
	}
	e.reconcile()
	e.deviceChanged(req.ID)
	e.candidatesChanged(req.ID)
	return nil
}
