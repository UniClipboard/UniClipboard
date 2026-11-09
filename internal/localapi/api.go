// Package api is the control interface of the daemon, spoken over a local
// unix socket the way docker's CLI talks to dockerd: plain HTTP and JSON, so
// the CLI, the GUI and curl --unix-socket are all equal clients.
package api

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/uniclipboard/unic/engine"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// PeerStatus is a paired device and whether it is connected right now.
type PeerStatus struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Connected bool   `json:"connected"`
	LastSeen  int64  `json:"last_seen,omitempty"`
	// Kind is "device" (omitted) or "external": a channel of a backend whose
	// peers cannot prove an identity. Channels have no fingerprint; the other
	// fields below are set for them only.
	Kind             string `json:"kind,omitempty"`
	BackendID        string `json:"backend_id,omitempty"`
	Enabled          bool   `json:"enabled,omitempty"` // Connected is false for channels; Enabled says whether they sync
	AutoApply        bool   `json:"auto_apply,omitempty"`
	RemoteRevocation bool   `json:"remote_revocation,omitempty"` // removal also leaves the remote group; otherwise it is local only
	Revision         int64  `json:"revision,omitempty"`
}

// ChannelUpdate is the body of PATCH /v1/channels/{id}. Revision is required.
type ChannelUpdate struct {
	Revision  int64   `json:"revision"`
	Name      *string `json:"name,omitempty"`
	Enabled   *bool   `json:"enabled,omitempty"`
	AutoApply *bool   `json:"auto_apply,omitempty"`
}

// Candidate is a device that a paired device vouched for (see docs/architecture/identity-and-trust.md). It
// is a suggestion: Name is not verified and pairing needs approval on both
// devices.
type Candidate struct {
	ID               string   `json:"id"`
	BackendID        string   `json:"backend_id"`
	Name             string   `json:"name"`
	IntroducedBy     []string `json:"introduced_by"`
	LastSeen         int64    `json:"last_seen"`
	Ignored          bool     `json:"ignored,omitempty"`
	Requested        bool     `json:"requested,omitempty"`
	RequestedUntilMs int64    `json:"requested_until_ms,omitempty"`
}

// PendingPeer is a device waiting for the user's approval.
type PendingPeer struct {
	ID   string `json:"id"`
	Name string `json:"name"`
	Via  string `json:"via,omitempty"` // who introduced it; empty if it connected to us directly
	// ByCode is true when it connected with the pairing code this device showed.
	ByCode bool `json:"by_code,omitempty"`
	// Fingerprint is shown on both devices so the user can check that the
	// request comes from the device they think it does.
	Fingerprint string            `json:"fingerprint"`
	Time        int64             `json:"time"`
	Revision    int64             `json:"revision"`
	Bindings    []BindingApproval `json:"bindings"`
}

// Approval carries the fingerprint and revisions displayed to the user.
type Approval struct {
	Fingerprint string            `json:"fingerprint"`
	Revision    int64             `json:"revision"`
	Bindings    []BindingApproval `json:"bindings"`
}
type BindingApproval struct {
	ID       string `json:"id"`
	Revision int64  `json:"revision"`
}

// Event is pushed to subscribers (the GUI). Info never carries clipboard
// content: only the kind of thing that moved.
type Event struct {
	Kind string `json:"kind"` // "peer-up", "peer-down", "sent", "received", "peers", "pending", "code", "candidates" (Info is a device id; fetch /v1/candidates), "config", "history" (Info is the record id; fetch it again, 404 means removed), "channel" (Info is the channel id; "peers" follows)
	Peer string `json:"peer,omitempty"`
	Info string `json:"info,omitempty"`
	Time int64  `json:"time"`
}

// Status is the reply of GET /v1/status.
type Status struct {
	ID    string       `json:"id"`
	Name  string       `json:"name"`
	Peers []PeerStatus `json:"peers"`
	// Pending are devices waiting for the user to approve them.
	Pending []PendingPeer `json:"pending"`
}

// PairResult is the reply of POST /v1/peers.
type PairResult struct {
	PeerStatus
	// Awaiting is true when the other device has not approved this one yet.
	Awaiting bool `json:"awaiting_approval"`
	// Fingerprint is what the other device shows next to this one's request.
	Fingerprint string `json:"fingerprint"`
}

// Code is the reply of GET and POST /v1/code: the pairing code this device
// shows, "NNN-NNN", valid until ExpiresAtMs (unix milliseconds).
type Code struct {
	Code        string `json:"code"`
	ExpiresAtMs int64  `json:"expires_at_ms"`
}

// Expires is ExpiresAtMs as a time.
func (c Code) Expires() time.Time { return time.UnixMilli(c.ExpiresAtMs) }

// Display renders the code as "NNN NNN".
func (c Code) Display() string { return strings.Replace(c.Code, "-", " ", 1) }

type ClipboardView struct {
	Summary string   `json:"summary"`
	Text    string   `json:"text,omitempty"`
	Formats []string `json:"formats"`
	Files   []string `json:"files,omitempty"`
}

// SetClipboard is the body of PUT /v1/clipboard: text, or absolute file paths.
type SetClipboard struct {
	Text  *string  `json:"text,omitempty"`
	Files []string `json:"files,omitempty"`
}

// HistoryPart describes one stored part; names and paths are not included.
type HistoryPart struct {
	Available bool   `json:"available"`
	Kind      string `json:"kind"` // "representation" or "file"
	MIME      string `json:"mime,omitempty"`
	Size      int64  `json:"size"`
}

// HistoryBackendSync is one sync backend's part in a record: what was decided,
// how the targets fared, and a stable failure class. It never says the other
// side received or applied the clip.
type HistoryBackendSync struct {
	BackendID   string `json:"backend_id"`
	State       string `json:"state"`       // not_requested, pending, sent, partial, failed, interrupted
	Disposition string `json:"disposition"` // the decision at commit, e.g. queued, policy_filtered
	ErrorCode   string `json:"error_code,omitempty"`
}

// HistoryItem is the local record of a clipboard entry, without its content.
type HistoryItem struct {
	ID         string               `json:"id"`
	Revision   uint64               `json:"revision"`
	Kind       string               `json:"kind"`   // "text", "image" or "files"
	Origin     string               `json:"origin"` // "local_system", "remote_device", "action", "explicit" or "external"
	DeviceID   string               `json:"device_id,omitempty"`
	DeviceName string               `json:"device_name,omitempty"`
	PluginID   string               `json:"plugin_id,omitempty"` // backend for origin "external", plugin for "action"
	CreatedAt  int64                `json:"created_at_ms"`
	UpdatedAt  int64                `json:"updated_at_ms"`
	ExpiresAt  int64                `json:"expires_at_ms"` // 0: never
	Tags       []string             `json:"tags"`
	Parts      []HistoryPart        `json:"parts"`
	TotalBytes int64                `json:"total_bytes"`
	SyncState  string               `json:"sync_state"`
	Backends   []HistoryBackendSync `json:"backends"`
}

// Action is something a plugin can do to a text record when asked.
type Action struct {
	Key      string   `json:"key"` // e.g. "builtin.text/upper"
	PluginID string   `json:"plugin_id"`
	Title    string   `json:"title"`
	Accepts  []string `json:"accepts"`
}

// HistoryQuery is GET /v1/history. Fields combine with AND.
type HistoryQuery struct {
	Limit    int
	Cursor   string
	Kinds    []string
	Origins  []string
	Tags     []string
	DeviceID string
	FromMs   int64
	BeforeMs int64
}

// HistoryPage is the reply of GET /v1/history. Revision changes whenever any
// record is added, changed or removed.
type HistoryPage struct {
	Items      []HistoryItem `json:"items"`
	NextCursor string        `json:"next_cursor,omitempty"`
	Revision   uint64        `json:"revision"`
}

// HistoryUpdate is the body of PATCH /v1/history/{id}. Revision is required;
// Tags replaces all tags, ExpiresAtMs 0 removes the expiry.
type HistoryUpdate struct {
	Revision    uint64    `json:"revision"`
	Tags        *[]string `json:"tags,omitempty"`
	ExpiresAtMs *int64    `json:"expires_at_ms,omitempty"`
}

// HistoryContent is an open part of a record. Close it.
type HistoryContent struct {
	MIME string
	Name string // file name; files only
	Size int64
	io.ReadCloser
}

type ErrorBody struct {
	Error string `json:"error"`
	// Reason classifies pairing code failures: "incomplete", "expired",
	// "invalid", "rate-limited" or "unavailable".
	Reason string `json:"reason,omitempty"`
}

// Error is an error reply from the daemon. A Backend returns it to choose the
// HTTP status; other errors are answered with 500.
type Error struct {
	Message string
	Reason  string // see errBody.Reason
	Status  int    // HTTP status when returned by a Backend
}

func (e *Error) Error() string { return e.Message }

// ReasonOf returns the reason of a daemon error, or "".
func ReasonOf(err error) string {
	var e *Error
	if errors.As(err, &e) {
		return e.Reason
	}
	var ee *engine.Error
	if errors.As(err, &ee) {
		return string(ee.Code)
	}
	return ""
}

// Backend is the local service protocol seam. The service host implements it
// on top of the engine; tests can implement it with a fake.
// FileCacheState adds desktop configuration to Engine's private-cache inventory.
type FileCacheState struct {
	engine.FileCacheState
	CustomDestination bool `json:"custom_destination"`
}

type Backend interface {
	Status(ctx context.Context) (Status, error)
	ExportTicket(ctx context.Context) (string, error)
	Pair(ctx context.Context, addr string) (PairResult, error)
	Code(ctx context.Context) (Code, bool, error)
	NewCode(ctx context.Context) (Code, error)
	CancelCode(ctx context.Context) error
	Join(ctx context.Context, code string) (PairResult, error)
	Pending(ctx context.Context) ([]PendingPeer, error)
	// Approve requires the fingerprint and the complete pending request snapshot.
	Approve(ctx context.Context, id string, approval Approval) (PeerStatus, error)
	Reject(ctx context.Context, id string) error
	// Candidates lists devices paired devices vouched for. PairCandidate asks
	// to pair with one (and keeps retrying while it is away); IgnoreCandidate
	// hides it or, with false, brings it back.
	Candidates(ctx context.Context) ([]Candidate, error)
	PairCandidate(ctx context.Context, id, backendID string) (PairResult, error)
	IgnoreCandidate(ctx context.Context, id string, ignored bool) error
	Unpair(ctx context.Context, id string) error
	// UpdateChannel changes an external channel; RemoveChannel removes it
	// (locally only unless PeerStatus.RemoteRevocation).
	UpdateChannel(ctx context.Context, id string, u ChannelUpdate) (PeerStatus, error)
	RemoveChannel(ctx context.Context, id string, revision int64) error
	Clipboard(ctx context.Context) (ClipboardView, error)
	SetClipboard(ctx context.Context, s SetClipboard) error
	ListHistory(ctx context.Context, q HistoryQuery) (HistoryPage, error)
	GetHistory(ctx context.Context, id string) (HistoryItem, error)
	// HistoryContent opens part number part (0-based) of a record.
	HistoryContent(ctx context.Context, id string, part int) (HistoryContent, error)
	// UseHistory puts a record on the system clipboard without syncing it or
	// creating a new record.
	UseHistory(ctx context.Context, id string) error
	// Actions lists what plugins offer, sorted by key.
	Actions(ctx context.Context) ([]Action, error)
	// RunAction creates a new history item without system application.
	// ExecuteAction exposes independent commit and optional application results.
	RunAction(ctx context.Context, id, key string) (HistoryItem, error)
	UpdateHistory(ctx context.Context, id string, u HistoryUpdate) (HistoryItem, error)
	DeleteHistory(ctx context.Context, id string, revision uint64) error
	// Subscribe streams events until the returned function is called.
	Subscribe() (<-chan Event, func())
}

// ---- client ----

type Client struct {
	hc   *http.Client
	Path string
}

// ErrNoDaemon means nothing listens on the socket.
var ErrNoDaemon = errors.New("the unic daemon is not running (start it with `unicd`)")

func NewClient(socket string) *Client {
	return &Client{Path: socket, hc: &http.Client{Transport: &http.Transport{
		DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
			var d net.Dialer
			return d.DialContext(ctx, "unix", socket)
		},
	}}}
}

func (c *Client) do(ctx context.Context, method, path string, in, out any) error {
	var body io.Reader
	if in != nil {
		b, err := json.Marshal(in)
		if err != nil {
			return err
		}
		body = bytes.NewReader(b)
	}
	req, err := http.NewRequestWithContext(ctx, method, "http://unic"+path, body)
	if err != nil {
		return err
	}
	resp, err := c.hc.Do(req)
	if err != nil {
		if isNoDaemon(err) {
			return ErrNoDaemon
		}
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != 200 {
		var e errBody
		_ = json.NewDecoder(resp.Body).Decode(&e)
		if e.Error == "" {
			e.Error = resp.Status
		}
		return &Error{Message: e.Error, Reason: e.Reason}
	}
	if out != nil {
		return json.NewDecoder(resp.Body).Decode(out)
	}
	return nil
}

func isNoDaemon(err error) bool {
	var oe *net.OpError
	return errors.As(err, &oe) && oe.Op == "dial"
}

func (c *Client) Status(ctx context.Context) (st Status, err error) {
	err = c.do(ctx, "GET", "/v1/status", nil, &st)
	return
}

// Config returns the configuration in effect.
func (c *Client) Config(ctx context.Context) (st config.State, err error) {
	err = c.do(ctx, "GET", "/v1/config", nil, &st)
	return
}

func (c *Client) Pair(ctx context.Context, addr string) (p PairResult, err error) {
	err = c.do(ctx, "POST", "/v1/peers", map[string]string{"addr": addr}, &p)
	return
}

// Code returns the pairing code this device shows; an error if none is live.
func (c *Client) Code(ctx context.Context) (v Code, err error) {
	err = c.do(ctx, "GET", "/v1/code", nil, &v)
	return
}

// NewCode registers a fresh pairing code, replacing the previous one.
func (c *Client) NewCode(ctx context.Context) (v Code, err error) {
	err = c.do(ctx, "POST", "/v1/code", nil, &v)
	return
}

// CancelCode withdraws the pairing code this device shows.
func (c *Client) CancelCode(ctx context.Context) error {
	return c.do(ctx, "DELETE", "/v1/code", nil, nil)
}

// Join pairs with the device showing code.
func (c *Client) Join(ctx context.Context, code string) (p PairResult, err error) {
	err = c.do(ctx, "POST", "/v1/join", map[string]string{"code": code}, &p)
	return
}

// Candidates lists the devices that paired devices vouched for.
func (c *Client) Candidates(ctx context.Context) (cs []Candidate, err error) {
	err = c.do(ctx, "GET", "/v1/candidates", nil, &cs)
	return
}

// PairCandidate asks to pair with a candidate.
func (c *Client) PairCandidate(ctx context.Context, id, backendID string) (p PairResult, err error) {
	err = c.do(ctx, "POST", "/v1/candidates/"+id+"/pair", map[string]string{"backend_id": backendID}, &p)
	return
}

// IgnoreCandidate hides a candidate, or restores it with ignored=false.
func (c *Client) IgnoreCandidate(ctx context.Context, id string, ignored bool) error {
	return c.do(ctx, "POST", "/v1/candidates/"+id+"/ignore", map[string]bool{"ignored": ignored}, nil)
}

func (c *Client) Pending(ctx context.Context) (ps []PendingPeer, err error) {
	err = c.do(ctx, "GET", "/v1/pending", nil, &ps)
	return
}

// Approve submits the exact request the user reviewed.
func (c *Client) Approve(ctx context.Context, id string, approval Approval) (p PeerStatus, err error) {
	err = c.do(ctx, "POST", "/v1/pending/"+id+"/approve", approval, &p)
	return
}

// ExportTicket explicitly obtains a secret, short-lived pairing ticket.
func (c *Client) ExportTicket(ctx context.Context) (string, error) {
	var out struct {
		Ticket string `json:"ticket"`
	}
	err := c.do(ctx, "POST", "/v1/pairing-ticket", nil, &out)
	return out.Ticket, err
}

func (c *Client) Reject(ctx context.Context, id string) error {
	return c.do(ctx, "POST", "/v1/pending/"+id+"/reject", nil, nil)
}

func (c *Client) Unpair(ctx context.Context, id string) error {
	return c.do(ctx, "DELETE", "/v1/peers/"+id, nil, nil)
}

func (c *Client) UpdateChannel(ctx context.Context, id string, u ChannelUpdate) (p PeerStatus, err error) {
	err = c.do(ctx, "PATCH", "/v1/channels/"+id, u, &p)
	return
}

func (c *Client) RemoveChannel(ctx context.Context, id string, revision int64) error {
	return c.do(ctx, "DELETE", "/v1/channels/"+id+"?revision="+strconv.FormatInt(revision, 10), nil, nil)
}

func (c *Client) Clipboard(ctx context.Context) (v ClipboardView, err error) {
	err = c.do(ctx, "GET", "/v1/clipboard", nil, &v)
	return
}

func (c *Client) SetClipboard(ctx context.Context, s SetClipboard) error {
	return c.do(ctx, "PUT", "/v1/clipboard", s, nil)
}

// Events calls fn for each event until ctx ends or the stream breaks.
func (c *Client) Events(ctx context.Context, fn func(Event)) error {
	req, err := http.NewRequestWithContext(ctx, "GET", "http://unic/v1/events", nil)
	if err != nil {
		return err
	}
	resp, err := c.hc.Do(req)
	if err != nil {
		if isNoDaemon(err) {
			return ErrNoDaemon
		}
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	sc := bufio.NewScanner(resp.Body)
	for sc.Scan() {
		line := sc.Text()
		if len(line) > 6 && line[:6] == "data: " {
			var ev Event
			if json.Unmarshal([]byte(line[6:]), &ev) == nil {
				fn(ev)
			}
		}
	}
	if ctx.Err() != nil {
		return nil
	}
	return sc.Err()
}

// SaveConfig replaces a complete TOML document using the revision of the last read.
func (c *Client) SaveConfig(ctx context.Context, revision string, raw []byte) (st config.State, err error) {
	req := struct {
		Revision string `json:"revision"`
		TOML     string `json:"toml"`
	}{revision, string(raw)}
	err = c.do(ctx, "PUT", "/v1/config", req, &st)
	return
}

func (c *Client) FileCache(ctx context.Context) (state FileCacheState, err error) {
	err = c.do(ctx, "GET", "/v1/file-cache", nil, &state)
	return
}

type errBody = ErrorBody
