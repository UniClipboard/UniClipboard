// Package host assembles Engine, local configuration and the UI-free socket service.
package host

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/app/service/internal/luascripts"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/plugins/builtinclassify"
	"github.com/uniclipboard/unic/internal/plugins/builtintext"
	"github.com/uniclipboard/unic/internal/plugins/tailcat"
	"github.com/uniclipboard/unic/internal/session"
)

type desktop interface {
	Read(context.Context) (clip.Snapshot, error)
	Send(context.Context, clip.Snapshot) error
	UseHistory(context.Context, engine.HistoryID) error
	RestoreHistory(context.Context, engine.HistoryID, string) (engine.RestoreSyncResult, error)
}

// Options configure a Node.
type Options struct {
	Dir      string          // the state directory (database, identity)
	Settings *config.Manager // the configuration file; edits reach the engine at once
	Logf     func(string, ...any)
	// Transport replaces the network of the built-in tailcat backend (tests);
	// nil uses the tailcat transport.
	Transport session.Transport
	// Plugins are extra plugin instances, e.g. sync backends with external
	// channels, assembled by the program next to the built-in ones.
	Plugins []engine.PluginInstance
	// LuaDir holds installed Lua plugins; the ones listed in the
	// configuration's plugins.lua run, and edits to their scripts are applied
	// while the node runs. Empty: no Lua plugins.
	LuaDir string
	// Observability supplies host-only test intervals or controlled credentials.
	Observability observability.Options
}

// Node owns Engine, local sessions and configuration. It implements
// api.Backend.
type Node struct {
	peer          *tailcat.Plugin
	diagnostics   *observability.Controller
	e             *engine.Engine
	cfg           *config.Manager
	hub           hub
	clip          desktop
	adapter       adapterState
	submissions   submissionState
	transfers     transferState
	runCtx        context.Context
	logf          func(string, ...any)
	customReceive bool

	luaDir string
	lua    map[string]luascripts.Script // as running, by plugin ID

	cancel context.CancelFunc
	wg     sync.WaitGroup
}

// Open creates the engine for the state directory. It does not touch the
// network; Start does.
func Open(ctx context.Context, o Options) (*Node, error) {
	// The host assembles the built-in backend explicitly.
	peer := tailcat.New()
	if o.Transport != nil {
		peer = tailcat.NewOn(o.Transport)
	}
	c := o.Settings.Current()
	storeReceived, err := receivedFileWriter(c.Inbox, o.Dir)
	if err != nil {
		return nil, err
	}
	obsOpts := o.Observability
	obsOpts.Dir = o.Dir
	obsOpts.Config = c.Observability()
	if o.Logf != nil {
		obsOpts.LocalOutput = func(record []byte) { o.Logf("%s", record) }
	}
	// Install the notification after Node exists. Its hub is allocated before
	// Engine.New, so diagnostics may report initialization without a GUI.
	n := &Node{peer: peer}
	obsOpts.Changed = func() { n.hub.emit(api.Event{Kind: "diagnostics"}) }
	diagnostics := observability.New(obsOpts)
	n.diagnostics = diagnostics
	peer.Observe(diagnostics)
	opened := false
	defer func() {
		if !opened {
			diagnostics.Close(context.Background())
		}
	}()
	opts := engine.Options{
		Logger: diagnostics.Logger(), Diagnostics: diagnostics.Record, ProductEvents: diagnostics.ProductEvent,
		DataDir:           o.Dir,
		CacheDir:          filepath.Join(o.Dir, "inbox", "file-cache"),
		StoreReceivedFile: storeReceived,
		DeferFileCleanup:  false,
		Settings:          toSettings(c),
		HostCapabilities: engine.HostCapabilities{
			Observe: engine.AbilityUnavailable, Apply: engine.AbilityUnavailable,
		},
		History: c.HistoryOptions(),
		// History remembers files by path; they are never copied.
		ResolveFileRef: resolveDesktopFile,
	}
	opts.Plugins = append(opts.Plugins, engine.PluginInstance{Plugin: peer, Builtin: true})
	if c.PluginEnabled(builtinclassify.ID) {
		onError, _ := c.ProcessorSettings(builtinclassify.ID)
		opts.Plugins = append(opts.Plugins, engine.PluginInstance{Plugin: builtinclassify.New(), OnError: onError})
	}
	if c.PluginEnabled(builtintext.ID) {
		onError, _ := c.ProcessorSettings(builtintext.ID)
		opts.Plugins = append(opts.Plugins, engine.PluginInstance{Plugin: builtintext.New(), OnError: onError})
	}
	opts.Plugins = append(opts.Plugins, o.Plugins...)
	var loaded map[string]luascripts.Script
	if o.LuaDir != "" {
		var specs []engine.LuaPluginSpec
		var lerr error
		if specs, loaded, lerr = luascripts.Specs(o.LuaDir, c.Plugins.Lua, c.ProcessorSettings); lerr != nil {
			return nil, lerr
		}
		opts.LuaPlugins = specs
	}
	e, err := engine.New(ctx, opts)
	if err != nil {
		return nil, err
	}
	diagnostics.SetSyncCollector(peer.Collect)
	n.e = e
	n.cfg = o.Settings
	n.logf = func(string, ...any) { diagnostics.Logger().Error("host.failed", "code", "internal") }
	n.luaDir = o.LuaDir
	n.lua = loaded
	n.customReceive = storeReceived != nil
	opened = true
	n.adapter.init(n)
	n.clip = &remoteDesktop{n: n}
	return n, nil
}

// toSettings maps the configuration file to engine settings. The environment
// variable that picks the pairing code service is read here, not in the engine.
func toSettings(c config.Config) engine.Settings {
	s := c.Settings()
	if s.RendezvousURL == "" {
		s.RendezvousURL = os.Getenv("UNIC_RENDEZVOUS_URL")
	}
	return s
}

// Start brings up networking and local state/event maintenance.
func (n *Node) Start(ctx context.Context) error {
	if err := n.e.Start(ctx); err != nil {
		return err
	}
	runCtx, cancel := context.WithCancel(context.Background())
	n.cancel = cancel
	n.runCtx = runCtx
	n.diagnostics.StartProduct("cli")
	n.wg.Add(2)
	if n.luaDir != "" && len(n.lua) > 0 {
		n.wg.Add(1)
		go func() { defer n.wg.Done(); n.watchLua(runCtx) }()
	}

	go func() { defer n.wg.Done(); n.pumpEvents(runCtx) }()
	go func() { defer n.wg.Done(); n.adapter.watch(runCtx) }()
	if runner, ok := n.clip.(interface{ Run(context.Context) }); ok {
		n.wg.Add(1)
		go func() { defer n.wg.Done(); runner.Run(runCtx) }()
	}
	return nil
}

// Close stops everything and releases the state directory.
func (n *Node) Close(ctx context.Context) error {
	n.peer.ExpectStop()
	if n.cancel != nil {
		n.cancel()
	}
	n.submissions.close()
	n.wg.Wait()
	n.adapter.close()
	err := n.e.Close(ctx)
	n.diagnostics.Close(context.Background())
	return err
}

// Engine exposes the engine, for tests.
func (n *Node) Engine() *engine.Engine { return n.e }

// ConfigChanged is called when the configuration file was reloaded (err is why
// it was rejected, if it was): the engine gets the new settings and
// subscribers are told.
func (n *Node) ConfigChanged(err error) {
	info := ""
	if err != nil {
		n.diagnostics.Logger().Warn("config.rejected")
		info = "configuration rejected"
	} else if uerr := n.applyConfiguration(); uerr != nil {
		n.cfg.ApplyFailed(uerr)
		n.logf("settings not applied: %v", asCode(uerr))
		info = "settings could not be applied"
	}
	if info == "" {
		n.cfg.ApplyFailed(nil)
	}
	n.hub.emit(api.Event{Kind: "config", Info: info})
}

func (n *Node) pushSettings(ctx context.Context) error {
	want := toSettings(n.cfg.Current())
	for i := 0; i < 5; i++ {
		st, err := n.e.GetState(ctx)
		if err != nil {
			return err
		}
		if reflect.DeepEqual(st.Settings.Value, want) {
			return nil
		}
		_, err = n.e.UpdateSettings(ctx, engine.UpdateSettingsRequest{ExpectedRevision: st.Settings.Revision, Value: want})
		var ee *engine.Error
		if !errors.As(err, &ee) || ee.Code != engine.CodeConflict {
			return err
		}
	}
	return errors.New("settings keep changing")
}

// ---- helpers ----

func randID() string {
	var b [8]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

// asCode is the error's stable code, for logs: its text may name things.
func asCode(err error) string {
	var ee *engine.Error
	if errors.As(err, &ee) {
		return string(ee.Code)
	}
	return "error"
}

func (n *Node) peerName(ctx context.Context, id engine.DeviceID) string {
	if d, err := n.e.GetDevice(ctx, string(id)); err == nil {
		return d.DisplayName()
	}
	return ""
}

// ---- api.Backend ----
//
// The daemon API still speaks of peers and pending requests. They are views of
// the engine's unified device list: a peer is a trusted device with an
// authorized path, a pending request is anything still waiting for the user.

func deviceConnected(d engine.Device) bool {
	for _, b := range d.Bindings {
		if b.Connection == engine.Connected {
			return true
		}
	}
	return false
}

func devicePeer(d engine.Device) bool {
	if d.Trust != engine.DeviceTrustTrusted {
		return false
	}
	for _, b := range d.Bindings {
		if b.State == engine.BindingAuthorized {
			return true
		}
	}
	return false
}

func devicePending(d engine.Device) bool {
	if d.Trust == engine.DeviceTrustPending {
		return true
	}
	for _, b := range d.Bindings {
		if b.State == engine.BindingPending {
			return true
		}
	}
	return false
}

func apiPeer(d engine.Device) api.PeerStatus {
	var last int64
	for _, b := range d.Bindings {
		if b.LastActiveMs > last {
			last = b.LastActiveMs
		}
	}
	return api.PeerStatus{ID: d.ID, Name: d.DisplayName(), Connected: deviceConnected(d), LastSeen: last / 1000}
}

func apiChannel(c engine.Channel) api.PeerStatus {
	return api.PeerStatus{ID: c.ID, Name: c.DisplayName(), LastSeen: c.LastActiveMs / 1000, Kind: "external", BackendID: string(c.BackendID),
		Enabled: c.Enabled, AutoApply: c.AutoApply, RemoteRevocation: c.RemoteRevocation, Revision: c.Revision}
}

func (n *Node) channelName(ctx context.Context, id string) string {
	if c, err := n.e.GetChannel(ctx, id); err == nil {
		return c.DisplayName()
	}
	return ""
}

func (n *Node) UpdateChannel(ctx context.Context, id string, u api.ChannelUpdate) (api.PeerStatus, error) {
	rev := u.Revision
	var c engine.Channel
	var err error
	// Each change bumps the revision, so chain the results.
	if u.Name != nil {
		if c, err = n.e.RenameChannel(ctx, engine.RenameChannelRequest{ID: id, ExpectedRevision: rev, Name: *u.Name}); err != nil {
			return api.PeerStatus{}, apiErr(err, false)
		}
		rev = c.Revision
	}
	if u.Enabled != nil {
		if c, err = n.e.SetChannelEnabled(ctx, engine.SetChannelEnabledRequest{ID: id, ExpectedRevision: rev, Enabled: *u.Enabled}); err != nil {
			return api.PeerStatus{}, apiErr(err, false)
		}
		rev = c.Revision
	}
	if u.AutoApply != nil {
		if c, err = n.e.SetChannelAutoApply(ctx, engine.SetChannelAutoApplyRequest{ID: id, ExpectedRevision: rev, AutoApply: *u.AutoApply}); err != nil {
			return api.PeerStatus{}, apiErr(err, false)
		}
	}
	if u.Name == nil && u.Enabled == nil && u.AutoApply == nil {
		return api.PeerStatus{}, &api.Error{Message: "nothing to change", Status: 400}
	}
	return apiChannel(c), nil
}

func (n *Node) RemoveChannel(ctx context.Context, id string, revision int64) error {
	return apiErr0(n.e.RemoveChannel(ctx, engine.RemoveChannelRequest{ID: id, ExpectedRevision: revision}))
}

func apiPending(d engine.Device) api.PendingPeer {
	byTicket := false
	for _, b := range d.Bindings {
		byTicket = byTicket || b.Ticket
	}
	out := api.PendingPeer{ID: d.ID, Name: d.DisplayName(), ByCode: byTicket, Fingerprint: d.Fingerprint, Time: d.UpdatedMs / 1000, Revision: d.Revision, Bindings: []api.BindingApproval{}}
	for _, b := range d.Bindings {
		if b.State == engine.BindingPending {
			out.Bindings = append(out.Bindings, api.BindingApproval{ID: b.ID, Revision: b.Revision})
		}
	}
	return out
}

// apiErr maps an engine error to the HTTP status and reason the API has always
// used. viaCode selects the pairing code service's classification.
func apiErr(err error, viaCode bool) error {
	var ee *engine.Error
	if !errors.As(err, &ee) {
		return &api.Error{Message: "internal error", Status: 500}
	}
	e := &api.Error{Message: ee.Message, Status: 502, Reason: string(ee.Code)}
	switch ee.Code {
	case engine.CodeIncomplete:
		e.Status, e.Reason = 400, "incomplete"
	case engine.CodeInvalid:
		e.Status, e.Reason = 400, "invalid"
	case engine.CodeExpired:
		e.Status, e.Reason = 404, "expired"
	case engine.CodeRateLimited:
		e.Status, e.Reason = 429, "rate-limited"
	case engine.CodeUnavailable:
		e.Status = 503
		if viaCode {
			e.Status, e.Reason = 503, "unavailable"
		}
	case engine.CodeInvalidArgument:
		e.Status = 400
	case engine.CodeUnsupported:
		e.Status = 501
	case engine.CodePermissionDenied:
		e.Status = 403
	case engine.CodeInUse:
		e.Status = 409
	case engine.CodeResourceExhausted:
		e.Status = 429
	case engine.CodeResourceExpired:
		e.Status = 410
	case engine.CodeSourceChanged:
		e.Status = 409
	case engine.CodeTimeout:
		e.Status = 504
	case engine.CodeNotFound:
		e.Status = 404
	case engine.CodeConflict, engine.CodeIdentityMismatch:
		e.Status = 409
	case engine.CodeInvalidState, engine.CodeSuspended, engine.CodeClosed:
		e.Status = 503
	}
	return e
}

func (n *Node) Status(ctx context.Context) (api.Status, error) {
	st, err := n.e.GetState(ctx)
	if err != nil {
		return api.Status{}, apiErr(err, false)
	}
	out := api.Status{ID: string(st.Self.ID), Name: st.Self.Name, Peers: []api.PeerStatus{}, Pending: []api.PendingPeer{}}
	ps, err := n.e.ListPeers(ctx) // devices and external channels, one list
	if err != nil {
		return api.Status{}, apiErr(err, false)
	}
	for _, p := range ps {
		switch {
		case p.Channel != nil:
			out.Peers = append(out.Peers, apiChannel(*p.Channel))
		case p.Device != nil && devicePeer(*p.Device):
			out.Peers = append(out.Peers, apiPeer(*p.Device))
		}
		if p.Device != nil && devicePending(*p.Device) {
			out.Pending = append(out.Pending, apiPending(*p.Device))
		}
	}
	return out, nil
}

// await waits for a pairing operation and turns its result into the reply.
func (n *Node) await(ctx context.Context, id engine.OperationID, viaCode bool) (api.PairResult, error) {
	for {
		op, err := n.e.GetOperation(ctx, id)
		if err != nil {
			return api.PairResult{}, apiErr(err, viaCode)
		}
		if op.Done() {
			if op.Phase == engine.PhaseFailed {
				return api.PairResult{}, apiErr(op.Error, viaCode)
			}
			res := api.PairResult{Awaiting: op.Pairing.AwaitingRemoteApproval, Fingerprint: op.Pairing.Fingerprint}
			res.ID, res.Name, res.Connected = string(op.Pairing.PeerID), n.peerName(ctx, op.Pairing.PeerID), !res.Awaiting
			return res, nil
		}
		select {
		case <-ctx.Done():
			return api.PairResult{}, apiErr(ctxError(ctx.Err()), viaCode)
		case <-time.After(20 * time.Millisecond):
		}
	}
}

func ctxError(err error) error {
	return &engine.Error{Code: engine.CodeTimeout, Message: "the operation timed out"}
}

func (n *Node) Pair(ctx context.Context, addr string) (api.PairResult, error) {
	id, err := n.e.JoinByTicket(ctx, engine.JoinTicketRequest{RequestID: engine.RequestID("pair-" + randID()), Ticket: addr})
	if err != nil {
		return api.PairResult{}, apiErr(err, false)
	}
	return n.await(ctx, id, false)
}

func (n *Node) Join(ctx context.Context, code string) (api.PairResult, error) {
	id, err := n.e.JoinByCode(ctx, engine.JoinRequest{RequestID: engine.RequestID("join-" + randID()), Code: code})
	if err != nil {
		return api.PairResult{}, apiErr(err, true)
	}
	return n.await(ctx, id, true)
}

func (n *Node) Code(ctx context.Context) (api.Code, bool, error) {
	c, ok, err := n.e.GetPairingCode(ctx)
	if err != nil || !ok {
		return api.Code{}, false, apiErr(err, true)
	}
	return api.Code{Code: c.Code, ExpiresAtMs: c.ExpiresAtMs}, true, nil
}

func (n *Node) NewCode(ctx context.Context) (api.Code, error) {
	c, err := n.e.CreatePairingCode(ctx)
	if err != nil {
		return api.Code{}, apiErr(err, true)
	}
	return api.Code{Code: c.Code, ExpiresAtMs: c.ExpiresAtMs}, nil
}

func (n *Node) CancelCode(ctx context.Context) error {
	return apiErr0(n.e.CancelPairingCode(ctx))
}

func apiErr0(err error) error {
	if err == nil {
		return nil
	}
	return apiErr(err, false)
}

func (n *Node) Pending(ctx context.Context) ([]api.PendingPeer, error) {
	ds, err := n.e.ListDevices(ctx)
	if err != nil {
		return nil, apiErr(err, false)
	}
	out := make([]api.PendingPeer, 0, len(ds))
	for _, d := range ds {
		if devicePending(d) {
			out = append(out, apiPending(d))
		}
	}
	return out, nil
}

// ExportTicket is an explicit request for a secret pairing ticket.
func (n *Node) ExportTicket(ctx context.Context) (string, error) {
	t, err := n.e.ExportPairingTicket(ctx)
	if err != nil {
		return "", apiErr(err, false)
	}
	return t.Ticket, nil
}

func (n *Node) Approve(ctx context.Context, id string, approval api.Approval) (api.PeerStatus, error) {
	req := engine.ApproveDeviceRequest{ID: id, ExpectedRevision: approval.Revision, ExpectedFingerprint: approval.Fingerprint}
	for _, b := range approval.Bindings {
		req.Bindings = append(req.Bindings, engine.BindingRequest{ID: b.ID, ExpectedRevision: b.Revision})
	}
	d, err := n.e.ApproveDevice(ctx, req)
	if err != nil {
		return api.PeerStatus{}, apiErr(err, false)
	}
	return apiPeer(d), nil
}

// Reject and Unpair both end the device: nothing is trusted and no path stays.
func (n *Node) Candidates(ctx context.Context) ([]api.Candidate, error) {
	cs, err := n.e.ListCandidates(ctx)
	if err != nil {
		return nil, apiErr(err, false)
	}
	out := make([]api.Candidate, len(cs))
	for i, c := range cs {
		by := make([]string, len(c.Introducers))
		for j, in := range c.Introducers {
			by[j] = in.Name
		}
		out[i] = api.Candidate{ID: c.DeviceID, BackendID: string(c.BackendID), Name: c.Name, IntroducedBy: by, LastSeen: c.LastSeenMs / 1000,
			Ignored: c.Ignored, Requested: c.Requested, RequestedUntilMs: c.RequestedUntilMs}
	}
	return out, nil
}

func (n *Node) PairCandidate(ctx context.Context, id, backendID string) (api.PairResult, error) {
	op, err := n.e.PairCandidate(ctx, engine.PairCandidateRequest{RequestID: engine.RequestID("cand-" + randID()), DeviceID: id, BackendID: engine.PluginID(backendID)})
	if err != nil {
		return api.PairResult{}, apiErr(err, false)
	}
	return n.await(ctx, op, false)
}

func (n *Node) IgnoreCandidate(ctx context.Context, id string, ignored bool) error {
	return apiErr0(n.e.IgnoreCandidate(ctx, id, ignored))
}

func (n *Node) Reject(ctx context.Context, id string) error { return n.Unpair(ctx, id) }

func (n *Node) Unpair(ctx context.Context, id string) error {
	d, err := n.e.GetDevice(ctx, id)
	if err != nil {
		return apiErr(err, false)
	}
	return apiErr0(n.e.UnpairDevice(ctx, engine.UnpairDeviceRequest{ID: d.ID, ExpectedRevision: d.Revision}))
}

func (n *Node) Clipboard(ctx context.Context) (api.ClipboardView, error) {
	s, err := n.clip.Read(ctx)
	if err != nil {
		return api.ClipboardView{}, apiErr0(err)
	}
	v := api.ClipboardView{Summary: s.Summary(), Formats: []string{}}
	for _, p := range s.Parts {
		v.Formats = append(v.Formats, p.Format)
		if p.Format == clip.Text {
			v.Text = string(p.Data)
		}
	}
	for _, f := range s.Files {
		v.Files = append(v.Files, f.Path)
	}
	return v, nil
}

func (n *Node) SetClipboard(ctx context.Context, b api.SetClipboard) error {
	var s clip.Snapshot
	switch {
	case b.Text != nil:
		s.Parts = []clip.Part{{Format: clip.Text, Data: []byte(*b.Text)}}
	case len(b.Files) > 0:
		for _, p := range b.Files {
			st, err := os.Stat(p)
			if err != nil {
				return &api.Error{Message: err.Error(), Status: 400}
			}
			if !st.Mode().IsRegular() {
				return &api.Error{Message: fmt.Sprintf("%s is not a regular file", p), Status: 400}
			}
			s.Files = append(s.Files, clip.File{Name: st.Name(), Path: p, Size: st.Size()})
		}
	default:
		return &api.Error{Message: "nothing to set", Status: 400}
	}
	if err := n.clip.Send(ctx, s); err != nil {
		return apiErr(err, false)
	}
	return nil
}

func (n *Node) Subscribe() (<-chan api.Event, func()) { return n.hub.subscribe() }

var _ api.Backend = (*Node)(nil)
var _ = strings.TrimSpace

func (n *Node) applyConfiguration() error {
	n.diagnostics.Apply(n.cfg.Current().Observability())
	n.diagnostics.Logger().Info("config.reloaded")
	return n.pushSettings(context.Background())
}

// Diagnostics is the host-owned controller, also used by the nonvisual E2E entry.
func (n *Node) Diagnostics() *observability.Controller { return n.diagnostics }
