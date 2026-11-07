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
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/config"
)

// PeerStatus is a paired device and whether it is connected right now.
type PeerStatus struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Connected bool   `json:"connected"`
	LastSeen  int64  `json:"last_seen,omitempty"`
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
	Fingerprint string `json:"fingerprint"`
	Time        int64  `json:"time"`
}

// Event is pushed to subscribers (the GUI). Info never carries clipboard
// content: only the kind of thing that moved.
type Event struct {
	Kind string `json:"kind"` // "peer-up", "peer-down", "sent", "received", "peers", "pending", "code", "config"
	Peer string `json:"peer,omitempty"`
	Info string `json:"info,omitempty"`
	Time int64  `json:"time"`
}

// Status is the reply of GET /v1/status.
type Status struct {
	ID    string       `json:"id"`
	Name  string       `json:"name"`
	Addr  string       `json:"addr"`
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

type errBody struct {
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
	return ""
}

// Backend is what the daemon does for the API. The desktop host implements it
// on top of the engine; tests can implement it with a fake.
type Backend interface {
	Status(ctx context.Context) (Status, error)
	Pair(ctx context.Context, addr string) (PairResult, error)
	Code(ctx context.Context) (Code, bool, error)
	NewCode(ctx context.Context) (Code, error)
	CancelCode(ctx context.Context) error
	Join(ctx context.Context, code string) (PairResult, error)
	Pending(ctx context.Context) ([]PendingPeer, error)
	// Approve trusts a pending device. A non-empty fingerprint must equal the
	// one the device is shown with now; this is how a client that displayed a
	// fingerprint makes sure the user approved that very request.
	Approve(ctx context.Context, id, fingerprint string) (PeerStatus, error)
	Reject(ctx context.Context, id string) error
	Unpair(ctx context.Context, id string) error
	Clipboard(ctx context.Context) (ClipboardView, error)
	SetClipboard(ctx context.Context, s SetClipboard) error
	// Subscribe streams events until the returned function is called.
	Subscribe() (<-chan Event, func())
}

// Handler serves the API for b.
func Handler(b Backend, cfg *config.Manager) http.Handler {
	mux := http.NewServeMux()
	reply := func(w http.ResponseWriter, code int, v any) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(code)
		_ = json.NewEncoder(w).Encode(v)
	}
	// fail answers with the Backend's chosen status, or def.
	fail := func(w http.ResponseWriter, def int, err error) {
		code := def
		var ae *Error
		if errors.As(err, &ae) && ae.Status != 0 {
			code = ae.Status
		}
		reply(w, code, errBody{Error: err.Error(), Reason: ReasonOf(err)})
	}

	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		st, err := b.Status(r.Context())
		if err != nil {
			fail(w, 500, err)
			return
		}
		if st.Peers == nil {
			st.Peers = []PeerStatus{}
		}
		if st.Pending == nil {
			st.Pending = []PendingPeer{}
		}
		reply(w, 200, st)
	})

	// The configuration in effect, where it lives, and why the last edit was
	// rejected (if it was).
	mux.HandleFunc("GET /v1/config", func(w http.ResponseWriter, r *http.Request) {
		reply(w, 200, cfg.State())
	})

	mux.HandleFunc("POST /v1/peers", func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Addr string `json:"addr"`
		}
		if err := json.NewDecoder(io.LimitReader(r.Body, 64<<10)).Decode(&body); err != nil || body.Addr == "" {
			fail(w, 400, errors.New(`body must be {"addr": "..."}`))
			return
		}
		ctx, cancel := context.WithTimeout(r.Context(), 45*time.Second)
		defer cancel()
		res, err := b.Pair(ctx, body.Addr)
		if err != nil {
			fail(w, 502, err)
			return
		}
		reply(w, 200, res)
	})

	// The pairing code this device shows, if one is live.
	mux.HandleFunc("GET /v1/code", func(w http.ResponseWriter, r *http.Request) {
		c, ok, err := b.Code(r.Context())
		switch {
		case err != nil:
			fail(w, 500, err)
		case !ok:
			fail(w, 404, errors.New("no pairing code is being shown"))
		default:
			reply(w, 200, c)
		}
	})

	// A fresh pairing code, replacing the previous one.
	mux.HandleFunc("POST /v1/code", func(w http.ResponseWriter, r *http.Request) {
		c, err := b.NewCode(r.Context())
		if err != nil {
			fail(w, 502, err)
			return
		}
		reply(w, 200, c)
	})

	mux.HandleFunc("DELETE /v1/code", func(w http.ResponseWriter, r *http.Request) {
		if err := b.CancelCode(r.Context()); err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})

	// Pair with the device showing a code.
	mux.HandleFunc("POST /v1/join", func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Code string `json:"code"`
		}
		if err := json.NewDecoder(io.LimitReader(r.Body, 4<<10)).Decode(&body); err != nil {
			fail(w, 400, errors.New(`body must be {"code": "NNN NNN"}`))
			return
		}
		ctx, cancel := context.WithTimeout(r.Context(), 45*time.Second)
		defer cancel()
		res, err := b.Join(ctx, body.Code)
		if err != nil {
			fail(w, 502, err)
			return
		}
		reply(w, 200, res)
	})

	mux.HandleFunc("GET /v1/pending", func(w http.ResponseWriter, r *http.Request) {
		ps, err := b.Pending(r.Context())
		if err != nil {
			fail(w, 500, err)
			return
		}
		if ps == nil {
			ps = []PendingPeer{}
		}
		reply(w, 200, ps)
	})

	// The body is optional: {"fingerprint": "..."} makes the approval fail with
	// 409 unless the device is still shown with that fingerprint.
	mux.HandleFunc("POST /v1/pending/{id}/approve", func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Fingerprint string `json:"fingerprint"`
		}
		if r.ContentLength != 0 {
			_ = json.NewDecoder(io.LimitReader(r.Body, 4<<10)).Decode(&body)
		}
		p, err := b.Approve(r.Context(), r.PathValue("id"), body.Fingerprint)
		if err != nil {
			fail(w, 404, err)
			return
		}
		reply(w, 200, PeerStatus{ID: p.ID, Name: p.Name})
	})

	mux.HandleFunc("POST /v1/pending/{id}/reject", func(w http.ResponseWriter, r *http.Request) {
		if err := b.Reject(r.Context(), r.PathValue("id")); err != nil {
			fail(w, 404, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})

	mux.HandleFunc("DELETE /v1/peers/{id}", func(w http.ResponseWriter, r *http.Request) {
		if err := b.Unpair(r.Context(), r.PathValue("id")); err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})

	mux.HandleFunc("GET /v1/clipboard", func(w http.ResponseWriter, r *http.Request) {
		v, err := b.Clipboard(r.Context())
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, v)
	})

	mux.HandleFunc("PUT /v1/clipboard", func(w http.ResponseWriter, r *http.Request) {
		var body SetClipboard
		if err := json.NewDecoder(io.LimitReader(r.Body, 64<<20)).Decode(&body); err != nil {
			fail(w, 400, err)
			return
		}
		if err := b.SetClipboard(r.Context(), body); err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})

	// Server-sent events: one JSON Event per "data:" line.
	mux.HandleFunc("GET /v1/events", func(w http.ResponseWriter, r *http.Request) {
		fl, ok := w.(http.Flusher)
		if !ok {
			fail(w, 500, errors.New("streaming unsupported"))
			return
		}
		ch, stop := b.Subscribe()
		defer stop()
		w.Header().Set("Content-Type", "text/event-stream")
		w.Header().Set("Cache-Control", "no-cache")
		if _, err := fmt.Fprint(w, ": ok\n\n"); err != nil {
			return
		}
		fl.Flush()
		ping := time.NewTicker(20 * time.Second)
		defer ping.Stop()
		for {
			select {
			case <-r.Context().Done():
				return
			case <-ping.C:
				if _, err := fmt.Fprint(w, ": ping\n\n"); err != nil {
					return
				}
				fl.Flush()
			case ev := <-ch:
				data, _ := json.Marshal(ev)
				if _, err := fmt.Fprintf(w, "data: %s\n\n", data); err != nil {
					return
				}
				fl.Flush()
			}
		}
	})
	return mux
}

// Listen creates the socket with owner-only permissions. A stale socket file
// from a crashed daemon is removed, but a live daemon is never displaced.
func Listen(path string) (net.Listener, error) {
	if c, err := net.DialTimeout("unix", path, time.Second); err == nil {
		_ = c.Close()
		return nil, fmt.Errorf("another unic daemon is already running (%s)", path)
	}
	_ = os.Remove(path)
	ln, err := net.Listen("unix", path)
	if err != nil {
		return nil, err
	}
	if err := os.Chmod(path, 0o600); err != nil {
		_ = ln.Close()
		return nil, err
	}
	return ln, nil
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

func (c *Client) Pending(ctx context.Context) (ps []PendingPeer, err error) {
	err = c.do(ctx, "GET", "/v1/pending", nil, &ps)
	return
}

// Approve trusts a pending device. Pass the fingerprint the user was shown so
// that the daemon refuses if the request has changed meanwhile; "" skips the check.
func (c *Client) Approve(ctx context.Context, id, fingerprint string) (p PeerStatus, err error) {
	var body any
	if fingerprint != "" {
		body = map[string]string{"fingerprint": fingerprint}
	}
	err = c.do(ctx, "POST", "/v1/pending/"+id+"/approve", body, &p)
	return
}

func (c *Client) Reject(ctx context.Context, id string) error {
	return c.do(ctx, "POST", "/v1/pending/"+id+"/reject", nil, nil)
}

func (c *Client) Unpair(ctx context.Context, id string) error {
	return c.do(ctx, "DELETE", "/v1/peers/"+id, nil, nil)
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
