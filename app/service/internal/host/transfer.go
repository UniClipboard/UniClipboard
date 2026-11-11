package host

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"mime"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/tcnet"
	"github.com/uniclipboard/unic/internal/webtransfer"
)

// Temporary transfer (0022): content moves to or from a peer that has no unic
// device identity, over the protocol of the browser transfer page. Nothing here
// touches device trust, and the secrets (address, token, code, link) are never
// logged or put into events or errors.

const (
	transferLifetime = rendezvous.MaxLifetime // a code never outlives this, so neither does an offer
	transferKeep     = 5 * time.Minute        // how long a finished transfer stays queryable
	receiveTimeout   = 90 * time.Second
)

type transfer struct {
	mu     sync.Mutex
	api    api.Transfer
	cancel context.CancelFunc
	ended  time.Time
}

type transferState struct {
	mu sync.Mutex
	m  map[string]*transfer
}

func (s *transferState) prune() {
	for id, t := range s.m {
		t.mu.Lock()
		old := !t.ended.IsZero() && time.Since(t.ended) > transferKeep
		t.mu.Unlock()
		if old {
			delete(s.m, id)
		}
	}
}

func transferErr(status int, reason, msg string) error {
	return &api.Error{Message: msg, Status: status, Reason: reason}
}

// snapshotItems turns a snapshot into protocol items. Text and PNG travel as
// themselves; files are read whole, because the protocol hashes every item.
func snapshotItems(s clip.Snapshot) ([]webtransfer.Item, error) {
	var items []webtransfer.Item
	for _, p := range s.Parts {
		switch p.Format {
		case clip.Text:
			if len(p.Data) > 0 {
				items = append(items, webtransfer.Item{Meta: webtransfer.Meta{Kind: webtransfer.KindText, MIME: "text/plain;charset=utf-8"}, Data: p.Data})
			}
		case clip.PNG:
			items = append(items, webtransfer.Item{Meta: webtransfer.Meta{Kind: webtransfer.KindImage, Name: "image.png", MIME: clip.PNG}, Data: p.Data})
		}
	}
	for _, f := range s.Files {
		if !filepath.IsAbs(f.Path) {
			return nil, transferErr(400, "invalid_argument", "files must be absolute paths")
		}
		st, err := os.Stat(f.Path)
		if err != nil || !st.Mode().IsRegular() {
			return nil, transferErr(400, "invalid_argument", "a file is missing or not a regular file")
		}
		if st.Size() > webtransfer.MaxTransferBytes {
			return nil, transferErr(413, "limit_exceeded", "the content is over the 20 MiB transfer limit")
		}
		data, err := os.ReadFile(f.Path)
		if err != nil {
			return nil, transferErr(400, "invalid_argument", "cannot read a file")
		}
		m := mime.TypeByExtension(filepath.Ext(f.Name))
		if m == "" {
			m = "application/octet-stream"
		}
		items = append(items, webtransfer.Item{Meta: webtransfer.Meta{Kind: webtransfer.KindFile, Name: f.Name, MIME: m}, Data: data})
	}
	if len(items) == 0 {
		return nil, transferErr(400, "invalid_argument", "nothing to send")
	}
	return items, nil
}

func (n *Node) rendezvousClient() *rendezvous.Client {
	return rendezvous.NewWithURL(toSettings(n.cfg.Current()).RendezvousURL)
}

// StartTransfer listens on a fresh tailcat endpoint and offers the content to the
// first receiver that presents the token.
func (n *Node) StartTransfer(ctx context.Context, in api.StartTransferInput) (api.Transfer, error) {
	items, err := snapshotItems(in.Snapshot)
	if err != nil {
		return api.Transfer{}, err
	}
	offer, prepared, err := webtransfer.Prepare(items)
	if err != nil {
		return api.Transfer{}, transferErr(413, "limit_exceeded", "the content is over the transfer limit (20 MiB, 50 items)")
	}
	ln, err := tcnet.ListenEphemeral(func(string, ...any) {})
	if err != nil {
		return api.Transfer{}, transferErr(503, "unavailable", "cannot reach the relay network")
	}
	ticket := webtransfer.Ticket{Addr: ln.TailcatAddr(), Token: webtransfer.NewToken()}
	rv := n.rendezvousClient()
	var code rendezvous.Code
	if c, cerr := rv.CreateRaw(ctx, ticket.Encode()); cerr == nil {
		code = c
	}
	expires := time.Now().Add(transferLifetime)
	if !code.Expires.IsZero() && code.Expires.Before(expires) {
		expires = code.Expires
	}
	tctx, cancel := context.WithDeadline(n.runCtx, expires)
	t := &transfer{cancel: cancel, api: api.Transfer{
		ID: webtransfer.RandomID(12), State: api.TransferWaiting, Link: ticket.Link(), Code: code.Code,
		ExpiresAtMs: expires.UnixMilli(), Items: len(prepared), Bytes: offer.Total,
	}}
	n.transfers.mu.Lock()
	n.transfers.prune()
	if n.transfers.m == nil {
		n.transfers.m = map[string]*transfer{}
	}
	n.transfers.m[t.api.ID] = t
	n.transfers.mu.Unlock()
	first := t.api
	n.wg.Add(1)
	go func() {
		defer n.wg.Done()
		defer cancel()
		defer func() { _ = ln.Close() }()
		serr := webtransfer.Serve(tctx, ln, ticket.Token, offer, prepared, webtransfer.Events{Started: func(int64) { t.set(api.TransferSending, "") }})
		if code.Code != "" {
			cctx, ccancel := context.WithTimeout(context.Background(), 5*time.Second)
			rv.Consume(cctx, code.Code)
			ccancel()
		}
		switch {
		case serr == nil:
			t.set(api.TransferDelivered, "")
			n.hub.emit(api.Event{Kind: "sent", Info: "temporary transfer"})
		case errors.Is(serr, context.DeadlineExceeded):
			t.set(api.TransferExpired, "")
		case errors.Is(serr, context.Canceled):
			t.set(api.TransferCancelled, "")
		default:
			t.set(api.TransferFailed, transferReason(serr))
		}
	}()
	return first, nil
}

func (t *transfer) set(state, reason string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if !t.ended.IsZero() {
		return
	}
	t.api.State, t.api.Reason = state, reason
	if state != api.TransferSending {
		t.ended = time.Now()
	}
}

func transferReason(err error) string {
	switch {
	case errors.Is(err, webtransfer.ErrBusy):
		return "busy"
	case errors.Is(err, webtransfer.ErrCancelled):
		return "peer-cancelled"
	case errors.Is(err, webtransfer.ErrRejected):
		return "rejected"
	case errors.Is(err, webtransfer.ErrIntegrity):
		return "integrity"
	case errors.Is(err, webtransfer.ErrTooLarge):
		return "too-large"
	case errors.Is(err, webtransfer.ErrProtocol):
		return "protocol"
	}
	return "connection-lost"
}

func (n *Node) lookupTransfer(id string) (*transfer, error) {
	n.transfers.mu.Lock()
	defer n.transfers.mu.Unlock()
	n.transfers.prune()
	t := n.transfers.m[id]
	if t == nil {
		return nil, transferErr(404, "not_found", "no such transfer")
	}
	return t, nil
}

// Transfer reports a transfer's state. Link and code are not repeated.
func (n *Node) Transfer(_ context.Context, id string) (api.Transfer, error) {
	t, err := n.lookupTransfer(id)
	if err != nil {
		return api.Transfer{}, err
	}
	t.mu.Lock()
	defer t.mu.Unlock()
	out := t.api
	out.Link, out.Code = "", ""
	return out, nil
}

// CancelTransfer stops a transfer. A finished transfer is left as it ended.
func (n *Node) CancelTransfer(_ context.Context, id string) error {
	t, err := n.lookupTransfer(id)
	if err != nil {
		return err
	}
	t.cancel()
	return nil
}

// resolveTicket maps a code or a link to the sender's ticket.
func (n *Node) resolveTicket(ctx context.Context, source string) (webtransfer.Ticket, error) {
	source = strings.TrimSpace(source)
	if norm, ok := rendezvous.NormalizeCode(source); ok {
		raw, err := n.rendezvousClient().ResolveRaw(ctx, norm)
		if err != nil {
			var re *rendezvous.Error
			if errors.As(err, &re) {
				if re.Reason == rendezvous.Unavailable {
					return webtransfer.Ticket{}, transferErr(503, "unavailable", "the code service is unavailable")
				}
				msg := map[rendezvous.Reason]string{
					rendezvous.Incomplete: "enter all 6 digits of the code", rendezvous.Expired: "this code has expired or was already used; ask for a new one",
					rendezvous.RateLimited: "too many attempts; wait a moment", rendezvous.Invalid: "that code is not valid",
				}[re.Reason]
				return webtransfer.Ticket{}, &api.Error{Message: msg, Status: 400, Reason: string(re.Reason)}
			}
			return webtransfer.Ticket{}, transferErr(503, "unavailable", "the code service is unavailable")
		}
		if t, ok := webtransfer.ParseTicket(raw); ok {
			return t, nil
		}
		if _, ok := rendezvous.ParseTicket(raw); ok {
			return webtransfer.Ticket{}, transferErr(400, "pairing_code", "this code pairs a device; use `unic device add`")
		}
		return webtransfer.Ticket{}, transferErr(400, "invalid", "that code is not a transfer code")
	}
	if t, ok := webtransfer.ParseLink(source); ok {
		return t, nil
	}
	reason, msg := webtransfer.SourceProblem(source)
	return webtransfer.Ticket{}, transferErr(400, reason, msg)
}

// Receive pulls content from a temporary sender and stores it as received content
// with an external origin.
func (n *Node) Receive(ctx context.Context, in api.ReceiveInput) (api.ReceiveResult, error) {
	t, err := n.resolveTicket(ctx, in.Source)
	if err != nil {
		return api.ReceiveResult{}, err
	}
	dctx, cancel := context.WithTimeout(ctx, receiveTimeout)
	defer cancel()
	conn, err := tcnet.DialEphemeral(dctx, t.Addr, 1, func(string, ...any) {})
	if err != nil {
		if errors.Is(err, tcnet.ErrBadAddress) {
			return api.ReceiveResult{}, transferErr(400, "invalid", "that link is not a valid transfer link")
		}
		return api.ReceiveResult{}, transferErr(504, "timeout", "the sender did not answer; check that it is still waiting")
	}
	defer func() { _ = conn.Close() }()

	var inbox *engine.TransferInbox
	var layout []transferPlacement
	got, err := webtransfer.Pull(dctx, conn, t.Token, func(o webtransfer.Offer) error {
		var offer engine.TransferOffer
		offer, layout = planTransfer(o)
		var aerr error
		inbox, aerr = n.e.AdmitTransfer(dctx, offer)
		return aerr
	})
	if err != nil {
		if inbox != nil {
			inbox.Abort()
		}
		return api.ReceiveResult{}, receiveErr(err)
	}
	content, items, werr := placeTransfer(inbox.Dir, layout, got)
	if werr != nil {
		inbox.Abort()
		return api.ReceiveResult{}, transferErr(500, "internal", "cannot store the received files")
	}
	inbox.Complete(content)
	n.hub.emit(api.Event{Kind: "received", Info: "temporary transfer"})
	return api.ReceiveResult{Items: items}, nil
}

func receiveErr(err error) error {
	switch {
	case errors.Is(err, context.DeadlineExceeded):
		return transferErr(504, "timeout", "the transfer timed out")
	case errors.Is(err, context.Canceled):
		return transferErr(499, "cancelled", "cancelled")
	case errors.Is(err, webtransfer.ErrBusy):
		return transferErr(409, "busy", "the sender is already serving another receiver")
	case errors.Is(err, webtransfer.ErrTooLarge):
		return transferErr(413, "too-large", "the content is over the 20 MiB transfer limit")
	case errors.Is(err, webtransfer.ErrIntegrity):
		return transferErr(502, "integrity", "the content failed verification")
	case errors.Is(err, webtransfer.ErrCancelled), errors.Is(err, webtransfer.ErrRejected):
		return transferErr(502, "peer-cancelled", "the sender cancelled the transfer")
	}
	var ee *engine.Error
	if errors.As(err, &ee) {
		return apiErr(err, false)
	}
	return transferErr(502, "connection-lost", "the connection to the sender was lost")
}

// transferPlacement says where one offered item goes: a text or PNG representation,
// or a file. Several texts are joined; further PNGs become files, because a content
// holds one representation per format.
type transferPlacement struct {
	item int
	file bool
	name string
}

func planTransfer(o webtransfer.Offer) (engine.TransferOffer, []transferPlacement) {
	var parts []engine.TransferPart
	var plan []transferPlacement
	haveText, havePNG := -1, -1
	var textSize int64
	for i, m := range o.Items {
		mt := strings.ToLower(strings.TrimSpace(strings.SplitN(m.MIME, ";", 2)[0]))
		switch {
		case m.Kind == webtransfer.KindText && m.Size > 0:
			if haveText < 0 {
				haveText = len(parts)
				parts = append(parts, engine.TransferPart{MIME: engine.MimeText})
			}
			plan = append(plan, transferPlacement{item: i})
			textSize += m.Size + 1 // a newline joins texts
		case m.Kind == webtransfer.KindImage && mt == engine.MimePNG && havePNG < 0 && m.Size > 0:
			havePNG = len(parts)
			parts = append(parts, engine.TransferPart{MIME: engine.MimePNG, Size: m.Size})
			plan = append(plan, transferPlacement{item: i})
		default:
			name := safeName(m.Name, i)
			parts = append(parts, engine.TransferPart{File: true, Name: name, Size: m.Size})
			plan = append(plan, transferPlacement{item: i, file: true, name: name})
		}
	}
	if haveText >= 0 {
		parts[haveText].Size = textSize - 1
	}
	sum := sha256.New()
	for _, m := range o.Items {
		sum.Write([]byte(m.SHA256))
	}
	return engine.TransferOffer{TransferID: o.TransferID, View: hex.EncodeToString(sum.Sum(nil)), Parts: parts}, plan
}

func safeName(name string, i int) string {
	name = filepath.Base(strings.ReplaceAll(name, "\\", "/"))
	if name == "." || name == "/" || name == "" {
		name = fmt.Sprintf("file-%d", i+1)
	}
	return name
}

func placeTransfer(dir string, plan []transferPlacement, got []webtransfer.Item) (engine.TransferContent, []api.ReceivedItem, error) {
	var c engine.TransferContent
	var items []api.ReceivedItem
	var text bytes.Buffer
	used := map[string]bool{}
	for _, p := range plan {
		it := got[p.item]
		ri := api.ReceivedItem{Kind: string(it.Kind), Name: it.Name, MIME: it.MIME, Size: it.Size}
		switch {
		case p.file:
			name := p.name
			for used[name] {
				name = "dup-" + name
			}
			used[name] = true
			path := filepath.Join(dir, name)
			if err := os.WriteFile(path, it.Data, 0o600); err != nil {
				return c, nil, err
			}
			c.Files = append(c.Files, engine.TransferFile{Name: name, Path: path, Size: it.Size})
		case it.Kind == webtransfer.KindText:
			if text.Len() > 0 {
				text.WriteByte('\n')
			}
			text.Write(it.Data)
			ri.Text = string(it.Data)
		default:
			c.Representations = append(c.Representations, engine.TransferRepresentation{MIME: engine.MimePNG, Data: it.Data})
		}
		items = append(items, ri)
	}
	if text.Len() > 0 {
		c.Representations = append([]engine.TransferRepresentation{{MIME: engine.MimeText, Data: text.Bytes()}}, c.Representations...)
	}
	return c, items, nil
}
