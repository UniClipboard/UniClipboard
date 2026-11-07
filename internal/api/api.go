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

	"unic/internal/clip"
	"unic/internal/config"
	"unic/internal/engine"
	"unic/internal/rendezvous"
	"unic/internal/store"
)

// Status is the reply of GET /v1/status.
type Status struct {
	ID    string              `json:"id"`
	Name  string              `json:"name"`
	Addr  string              `json:"addr"`
	Peers []engine.PeerStatus `json:"peers"`
	// Pending are devices waiting for the user to approve them.
	Pending []engine.PendingPeer `json:"pending"`
}

// PairResult is the reply of POST /v1/peers.
type PairResult struct {
	engine.PeerStatus
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

// Error is an error reply from the daemon.
type Error struct {
	Message string
	Reason  string // see errBody.Reason
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

// Handler serves the API for e.
func Handler(e *engine.Engine, cfg *config.Manager) http.Handler {
	mux := http.NewServeMux()
	reply := func(w http.ResponseWriter, code int, v any) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(code)
		_ = json.NewEncoder(w).Encode(v)
	}
	fail := func(w http.ResponseWriter, code int, err error) {
		reply(w, code, errBody{Error: err.Error(), Reason: string(rendezvous.ReasonOf(err))})
	}
	// codeFail maps a pairing code failure to a status.
	codeFail := func(w http.ResponseWriter, err error) {
		switch rendezvous.ReasonOf(err) {
		case rendezvous.Incomplete, rendezvous.Invalid:
			fail(w, 400, err)
		case rendezvous.Expired:
			fail(w, 404, err)
		case rendezvous.RateLimited:
			fail(w, 429, err)
		case rendezvous.Unavailable:
			fail(w, 503, err)
		default:
			fail(w, 502, err)
		}
	}
	codeReply := func(w http.ResponseWriter, c rendezvous.Code) {
		reply(w, 200, Code{Code: c.Code, ExpiresAtMs: c.Expires.UnixMilli()})
	}
	pairReply := func(w http.ResponseWriter, p store.Peer, awaiting bool) {
		reply(w, 200, PairResult{PeerStatus: engine.PeerStatus{ID: p.ID, Name: p.Name, Connected: !awaiting},
			Awaiting: awaiting, Fingerprint: e.Fingerprint(p.ID)})
	}

	status := func() (Status, error) {
		id, name, addr := e.Self()
		peers, err := e.Peers()
		if peers == nil {
			peers = []engine.PeerStatus{}
		}
		pending, perr := e.Pending()
		if pending == nil {
			pending = []engine.PendingPeer{}
		}
		if err == nil {
			err = perr
		}
		return Status{ID: id, Name: name, Addr: addr, Peers: peers, Pending: pending}, err
	}

	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		st, err := status()
		if err != nil {
			fail(w, 500, err)
			return
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
		p, awaiting, err := e.AddPeer(ctx, body.Addr)
		if err != nil {
			fail(w, 502, err)
			return
		}
		pairReply(w, p, awaiting)
	})

	// The pairing code this device shows, if one is live.
	mux.HandleFunc("GET /v1/code", func(w http.ResponseWriter, r *http.Request) {
		c, ok := e.CurrentCode()
		if !ok {
			fail(w, 404, errors.New("no pairing code is being shown"))
			return
		}
		codeReply(w, c)
	})

	// A fresh pairing code, replacing the previous one.
	mux.HandleFunc("POST /v1/code", func(w http.ResponseWriter, r *http.Request) {
		c, err := e.IssueCode(r.Context())
		if err != nil {
			codeFail(w, err)
			return
		}
		codeReply(w, c)
	})

	mux.HandleFunc("DELETE /v1/code", func(w http.ResponseWriter, r *http.Request) {
		e.CancelCode()
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
		p, awaiting, err := e.Join(ctx, body.Code)
		if err != nil {
			codeFail(w, err)
			return
		}
		pairReply(w, p, awaiting)
	})

	mux.HandleFunc("GET /v1/pending", func(w http.ResponseWriter, r *http.Request) {
		ps, err := e.Pending()
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, ps)
	})

	mux.HandleFunc("POST /v1/pending/{id}/approve", func(w http.ResponseWriter, r *http.Request) {
		p, err := e.Approve(r.PathValue("id"))
		if err != nil {
			fail(w, 404, err)
			return
		}
		reply(w, 200, engine.PeerStatus{ID: p.ID, Name: p.Name})
	})

	mux.HandleFunc("POST /v1/pending/{id}/reject", func(w http.ResponseWriter, r *http.Request) {
		if err := e.Reject(r.PathValue("id")); err != nil {
			fail(w, 404, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})

	mux.HandleFunc("DELETE /v1/peers/{id}", func(w http.ResponseWriter, r *http.Request) {
		ok, err := e.RemovePeer(r.PathValue("id"))
		switch {
		case err != nil:
			fail(w, 500, err)
		case !ok:
			fail(w, 404, errors.New("no such peer"))
		default:
			reply(w, 200, map[string]bool{"ok": true})
		}
	})

	mux.HandleFunc("GET /v1/clipboard", func(w http.ResponseWriter, r *http.Request) {
		s, err := e.Current()
		if err != nil {
			fail(w, 500, err)
			return
		}
		v := ClipboardView{Summary: s.Summary(), Formats: []string{}}
		for _, p := range s.Parts {
			v.Formats = append(v.Formats, p.Format)
			if p.Format == clip.Text {
				v.Text = string(p.Data)
			}
		}
		for _, f := range s.Files {
			v.Files = append(v.Files, f.Path)
		}
		reply(w, 200, v)
	})

	mux.HandleFunc("PUT /v1/clipboard", func(w http.ResponseWriter, r *http.Request) {
		var body SetClipboard
		if err := json.NewDecoder(io.LimitReader(r.Body, 64<<20)).Decode(&body); err != nil {
			fail(w, 400, err)
			return
		}
		var s clip.Snapshot
		switch {
		case body.Text != nil:
			s.Parts = []clip.Part{{Format: clip.Text, Data: []byte(*body.Text)}}
		case len(body.Files) > 0:
			for _, p := range body.Files {
				st, err := os.Stat(p)
				if err != nil {
					fail(w, 400, err)
					return
				}
				if !st.Mode().IsRegular() {
					fail(w, 400, fmt.Errorf("%s is not a regular file", p))
					return
				}
				s.Files = append(s.Files, clip.File{Name: st.Name(), Path: p, Size: st.Size()})
			}
		default:
			fail(w, 400, errors.New("nothing to set"))
			return
		}
		if err := e.SetLocal(s); err != nil {
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
		ch, stop := e.Subscribe()
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
				b, _ := json.Marshal(ev)
				if _, err := fmt.Fprintf(w, "data: %s\n\n", b); err != nil {
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

func (c *Client) Pending(ctx context.Context) (ps []engine.PendingPeer, err error) {
	err = c.do(ctx, "GET", "/v1/pending", nil, &ps)
	return
}

func (c *Client) Approve(ctx context.Context, id string) (p engine.PeerStatus, err error) {
	err = c.do(ctx, "POST", "/v1/pending/"+id+"/approve", nil, &p)
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
func (c *Client) Events(ctx context.Context, fn func(engine.Event)) error {
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
			var ev engine.Event
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
