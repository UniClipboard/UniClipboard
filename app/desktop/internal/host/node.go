// Package host is the desktop host of the engine on a computer: it connects the
// system clipboard, the configuration file and the local HTTP API to the
// public engine package. Everything unic does on a desktop goes through here;
// the engine itself knows nothing of any of them.
package host

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/app/desktop/internal/config"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
)

// Board is the system clipboard as the desktop host sees it. The production
// implementation is backed by mygo (internal/board); tests use a fake. The
// engine never touches it: the host reads and writes it and tells the engine.
type Board interface {
	Read() (clip.Snapshot, error)
	Write(clip.Snapshot) error
}

// Options configure a Node.
type Options struct {
	Dir      string          // the state directory (database, identity)
	Settings *config.Manager // the configuration file; edits reach the engine at once
	Board    Board           // the system clipboard
	Poll     time.Duration   // clipboard poll interval; default 400ms
	Logf     func(string, ...any)
	// Network replaces tailcat (tests).
	Network engine.Network
}

// Node is the engine plus everything a desktop host adds to it. It implements
// api.Backend.
type Node struct {
	e    *engine.Engine
	cfg  *config.Manager
	hub  hub
	clip *clipboardHost
	logf func(string, ...any)

	cancel context.CancelFunc
	wg     sync.WaitGroup
}

// Open creates the engine for the state directory. It does not touch the
// network; Start does.
func Open(ctx context.Context, o Options) (*Node, error) {
	if o.Logf == nil {
		o.Logf = func(string, ...any) {}
	}
	if o.Poll == 0 {
		o.Poll = 400 * time.Millisecond
	}
	c := o.Settings.Current()
	opts := engine.Options{
		DataDir:  o.Dir,
		InboxDir: c.Inbox, // empty: the default inside Dir
		Settings: toSettings(c),
		HostCapabilities: engine.HostCapabilities{
			ReadFormats:  []string{clip.Text, clip.HTML, clip.PNG},
			WriteFormats: []string{clip.Text, clip.HTML, clip.PNG},
			ReadFiles:    true, WriteFiles: true,
			Observe: engine.AbilityAvailable, Apply: engine.AbilityAvailable,
		},
		Network: o.Network,
		History: c.HistoryOptions(),
		// History remembers files by path; they are never copied.
		ResolveFileRef: func(ref string) engine.FileSource { return pathSource{ref} },
	}
	e, err := engine.New(ctx, opts)
	if err != nil {
		return nil, err
	}
	n := &Node{e: e, cfg: o.Settings, logf: o.Logf}
	n.clip = &clipboardHost{e: e, board: o.Board, poll: o.Poll, hub: &n.hub, logf: o.Logf, name: n.peerName}
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

// Start brings the network up and starts watching the clipboard.
func (n *Node) Start(ctx context.Context) error {
	if err := n.e.Start(ctx); err != nil {
		return err
	}
	runCtx, cancel := context.WithCancel(context.Background())
	n.cancel = cancel
	n.wg.Add(2)
	go func() { defer n.wg.Done(); n.clip.Run(runCtx) }()
	go func() { defer n.wg.Done(); n.pumpEvents(runCtx) }()
	return nil
}

// Close stops everything and releases the state directory.
func (n *Node) Close(ctx context.Context) error {
	if n.cancel != nil {
		n.cancel()
	}
	n.wg.Wait()
	return n.e.Close(ctx)
}

// Engine exposes the engine, for tests.
func (n *Node) Engine() *engine.Engine { return n.e }

// ConfigChanged is called when the configuration file was reloaded (err is why
// it was rejected, if it was): the engine gets the new settings and
// subscribers are told.
func (n *Node) ConfigChanged(err error) {
	info := ""
	if err != nil {
		info = err.Error()
	} else if uerr := n.pushSettings(context.Background()); uerr != nil {
		n.logf("settings not applied: %v", asCode(uerr))
		info = "settings could not be applied"
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
		if st.Settings.Value == want {
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

func readPart(ctx context.Context, e *engine.Engine, lease engine.LeaseID, part engine.PartID) ([]byte, error) {
	r, err := e.ReadContent(ctx, lease, part)
	if err != nil {
		return nil, err
	}
	defer r.Close()
	return io.ReadAll(r)
}

func (n *Node) peerName(ctx context.Context, id engine.DeviceID) string {
	ps, err := n.e.ListPeers(ctx)
	if err == nil {
		for _, p := range ps {
			if p.ID == id {
				return p.Name
			}
		}
	}
	return ""
}

// ---- api.Backend ----

func apiPeer(p engine.Peer) api.PeerStatus {
	return api.PeerStatus{ID: string(p.ID), Name: p.Name, Connected: p.Connection == engine.Connected, LastSeen: p.LastSeenMs / 1000}
}

func apiPending(p engine.PendingPeer) api.PendingPeer {
	return api.PendingPeer{ID: string(p.ID), Name: p.Name, Via: p.IntroducedByName, ByCode: p.Source == engine.SourceCode,
		Fingerprint: p.Fingerprint, Time: p.SeenAtMs / 1000}
}

// apiErr maps an engine error to the HTTP status and reason the API has always
// used. viaCode selects the pairing code service's classification.
func apiErr(err error, viaCode bool) error {
	var ee *engine.Error
	if !errors.As(err, &ee) {
		return &api.Error{Message: "internal error", Status: 500}
	}
	e := &api.Error{Message: ee.Message, Status: 502}
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
		if viaCode {
			e.Status, e.Reason = 503, "unavailable"
		}
	case engine.CodeInvalidArgument:
		e.Status = 400
	case engine.CodeNotFound:
		e.Status = 404
	case engine.CodeConflict:
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
	if a, err := n.e.ExportPairingAddress(ctx); err == nil {
		out.Addr = a.Address
	}
	for _, p := range st.Peers {
		out.Peers = append(out.Peers, apiPeer(p))
	}
	for _, p := range st.Pending {
		out.Pending = append(out.Pending, apiPending(p))
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
	id, err := n.e.PairByAddress(ctx, engine.PairRequest{RequestID: engine.RequestID("pair-" + randID()), Address: addr})
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
	ps, err := n.e.ListPending(ctx)
	if err != nil {
		return nil, apiErr(err, false)
	}
	out := make([]api.PendingPeer, 0, len(ps))
	for _, p := range ps {
		out = append(out, apiPending(p))
	}
	return out, nil
}

func (n *Node) pendingFor(ctx context.Context, id string) (engine.PendingPeer, error) {
	ps, err := n.e.ListPending(ctx)
	if err != nil {
		return engine.PendingPeer{}, apiErr(err, false)
	}
	for _, p := range ps {
		if string(p.ID) == id {
			return p, nil
		}
	}
	return engine.PendingPeer{}, &api.Error{Message: "no such pending device", Status: 404}
}

func (n *Node) Approve(ctx context.Context, id, fingerprint string) (api.PeerStatus, error) {
	p, err := n.pendingFor(ctx, id)
	if err != nil {
		return api.PeerStatus{}, err
	}
	if fingerprint == "" {
		fingerprint = p.Fingerprint
	}
	peer, err := n.e.Approve(ctx, engine.ApprovalRequest{DeviceID: p.ID, RequestRevision: p.RequestRevision, ExpectedFingerprint: fingerprint})
	if err != nil {
		return api.PeerStatus{}, apiErr(err, false)
	}
	return apiPeer(peer), nil
}

func (n *Node) Reject(ctx context.Context, id string) error {
	p, err := n.pendingFor(ctx, id)
	if err != nil {
		return err
	}
	return apiErr0(n.e.Reject(ctx, engine.PendingRequest{DeviceID: p.ID, RequestRevision: p.RequestRevision}))
}

func (n *Node) Unpair(ctx context.Context, id string) error {
	ps, err := n.e.ListPeers(ctx)
	if err != nil {
		return apiErr(err, false)
	}
	for _, p := range ps {
		if string(p.ID) == id {
			return apiErr0(n.e.Unpair(ctx, p.ID))
		}
	}
	return &api.Error{Message: "no such peer", Status: 404}
}

func (n *Node) Clipboard(ctx context.Context) (api.ClipboardView, error) {
	s, err := n.clip.board.Read()
	if err != nil {
		return api.ClipboardView{}, err
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
