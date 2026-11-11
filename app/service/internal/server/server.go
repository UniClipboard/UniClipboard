package server

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"mime"
	"net"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// Handler serves the API for b.
func Handler(b api.Backend, cfg *config.Manager) http.Handler {
	mux := http.NewServeMux()
	mux.Handle("POST /v1/adapter", boundedRequests(adapterHandler(b), 8))
	mux.HandleFunc("GET /v1/adapter/content", adapterContentHandler(b))
	diagnosticRoutes(mux, b)
	headlessRoutes(mux, b)
	transferRoutes(mux, b)
	searchRoutes(mux, b)

	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		st, err := b.Status(r.Context())
		if err != nil {
			fail(w, 500, err)
			return
		}
		if st.Peers == nil {
			st.Peers = []api.PeerStatus{}
		}
		if st.Pending == nil {
			st.Pending = []api.PendingPeer{}
		}
		reply(w, 200, st)
	})

	// The configuration in effect, where it lives, and why the last edit was
	// rejected (if it was).
	mux.HandleFunc("PUT /v1/config", func(w http.ResponseWriter, r *http.Request) {
		var req struct {
			Revision string `json:"revision"`
			TOML     string `json:"toml"`
		}
		dec := json.NewDecoder(io.LimitReader(r.Body, 1<<20))
		dec.DisallowUnknownFields()
		if err := dec.Decode(&req); err != nil {
			fail(w, 400, errors.New("invalid configuration request"))
			return
		}
		state, err := cfg.Replace(req.Revision, []byte(req.TOML))
		if err != nil {
			code := 400
			if errors.Is(err, config.ErrConflict) {
				code = 409
			}
			fail(w, code, err)
			return
		}
		reply(w, 200, state)
	})
	mux.HandleFunc("GET /v1/file-cache", func(w http.ResponseWriter, r *http.Request) {
		if source, ok := b.(interface {
			FileCacheState(context.Context) (api.FileCacheState, error)
		}); ok {
			state, err := source.FileCacheState(r.Context())
			if err != nil {
				fail(w, 500, err)
				return
			}
			reply(w, 200, state)
			return
		}
		fail(w, 501, errors.New("file cache unavailable"))
	})
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

	mux.HandleFunc("GET /v1/candidates", func(w http.ResponseWriter, r *http.Request) {
		cs, err := b.Candidates(r.Context())
		if err != nil {
			fail(w, 500, err)
			return
		}
		if cs == nil {
			cs = []api.Candidate{}
		}
		reply(w, 200, cs)
	})

	mux.HandleFunc("POST /v1/candidates/{id}/pair", func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			BackendID string `json:"backend_id"`
		}
		if err := json.NewDecoder(io.LimitReader(r.Body, 4<<10)).Decode(&body); err != nil || body.BackendID == "" {
			fail(w, 400, errors.New(`body must be {"backend_id": "..."}`))
			return
		}
		ctx, cancel := context.WithTimeout(r.Context(), 45*time.Second)
		defer cancel()
		res, err := b.PairCandidate(ctx, r.PathValue("id"), body.BackendID)
		if err != nil {
			fail(w, 502, err)
			return
		}
		reply(w, 200, res)
	})

	mux.HandleFunc("POST /v1/candidates/{id}/ignore", func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Ignored bool `json:"ignored"`
		}
		if err := json.NewDecoder(io.LimitReader(r.Body, 4<<10)).Decode(&body); err != nil {
			fail(w, 400, errors.New(`body must be {"ignored": true|false}`))
			return
		}
		if err := b.IgnoreCandidate(r.Context(), r.PathValue("id"), body.Ignored); err != nil {
			fail(w, 400, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})

	mux.HandleFunc("GET /v1/pending", func(w http.ResponseWriter, r *http.Request) {
		ps, err := b.Pending(r.Context())
		if err != nil {
			fail(w, 500, err)
			return
		}
		if ps == nil {
			ps = []api.PendingPeer{}
		}
		reply(w, 200, ps)
	})

	mux.HandleFunc("POST /v1/pending/{id}/approve", func(w http.ResponseWriter, r *http.Request) {
		var body api.Approval
		dec := json.NewDecoder(io.LimitReader(r.Body, 64<<10))
		dec.DisallowUnknownFields()
		if err := dec.Decode(&body); err != nil || strings.TrimSpace(body.Fingerprint) == "" || body.Revision <= 0 || len(body.Bindings) == 0 {
			fail(w, 400, &api.Error{Status: 400, Message: "fingerprint, revision and pending bindings are required"})
			return
		}
		var extra any
		if err := dec.Decode(&extra); err != io.EOF {
			fail(w, 400, &api.Error{Status: 400, Message: "invalid approval body"})
			return
		}
		p, err := b.Approve(r.Context(), r.PathValue("id"), body)
		if err != nil {
			fail(w, 404, err)
			return
		}
		reply(w, 200, p)
	})

	mux.HandleFunc("POST /v1/pairing-ticket", func(w http.ResponseWriter, r *http.Request) {
		ticket, err := b.ExportTicket(r.Context())
		if err != nil {
			fail(w, 503, err)
			return
		}
		w.Header().Set("Cache-Control", "no-store")
		reply(w, 200, map[string]string{"ticket": ticket})
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

	mux.HandleFunc("PATCH /v1/channels/{id}", func(w http.ResponseWriter, r *http.Request) {
		var u api.ChannelUpdate
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096)).Decode(&u); err != nil || u.Revision == 0 {
			fail(w, 400, errors.New("revision is required"))
			return
		}
		p, err := b.UpdateChannel(r.Context(), r.PathValue("id"), u)
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, p)
	})

	mux.HandleFunc("DELETE /v1/channels/{id}", func(w http.ResponseWriter, r *http.Request) {
		rev, err := strconv.ParseInt(r.URL.Query().Get("revision"), 10, 64)
		if err != nil || rev == 0 {
			fail(w, 400, errors.New("revision is required"))
			return
		}
		if err := b.RemoveChannel(r.Context(), r.PathValue("id"), rev); err != nil {
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
		var body api.SetClipboard
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

	// ---- history (local only; events carry ids, never content) ----
	mux.HandleFunc("GET /v1/history/tags", func(w http.ResponseWriter, r *http.Request) {
		source, ok := b.(interface {
			ListHistoryTags(context.Context) ([]string, error)
		})
		if !ok {
			reply(w, http.StatusNotImplemented, map[string]string{"reason": "unsupported"})
			return
		}
		tags, err := source.ListHistoryTags(r.Context())
		if err != nil {
			fail(w, 500, err)
			return
		}
		if tags == nil {
			tags = []string{}
		}
		reply(w, 200, tags)
	})
	mux.HandleFunc("GET /v1/history", func(w http.ResponseWriter, r *http.Request) {
		v := r.URL.Query()
		q := api.HistoryQuery{Cursor: v.Get("cursor"), Kinds: v["kind"], Origins: v["origin"], Tags: v["tag"], DeviceID: v.Get("device")}
		for name, dst := range map[string]*int64{"from": &q.FromMs, "before": &q.BeforeMs} {
			if x := v.Get(name); x != "" {
				n, err := strconv.ParseInt(x, 10, 64)
				if err != nil {
					fail(w, 400, errors.New(name+": not a number"))
					return
				}
				*dst = n
			}
		}
		if x := v.Get("limit"); x != "" {
			n, err := strconv.Atoi(x)
			if err != nil {
				fail(w, 400, errors.New("limit: not a number"))
				return
			}
			q.Limit = n
		}
		page, err := b.ListHistory(r.Context(), q)
		if err != nil {
			fail(w, 500, err)
			return
		}
		if page.Items == nil {
			page.Items = []api.HistoryItem{}
		}
		reply(w, 200, page)
	})
	mux.HandleFunc("GET /v1/history/{id}", func(w http.ResponseWriter, r *http.Request) {
		it, err := b.GetHistory(r.Context(), r.PathValue("id"))
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, it)
	})
	// The bytes of one part: ?part=N (default 0). The file name, if any, is
	// only in the Content-Disposition header.
	mux.HandleFunc("GET /v1/history/{id}/content", func(w http.ResponseWriter, r *http.Request) {
		part := 0
		if x := r.URL.Query().Get("part"); x != "" {
			n, err := strconv.Atoi(x)
			if err != nil || n < 0 {
				fail(w, 400, errors.New("part: not a valid index"))
				return
			}
			part = n
		}
		c, err := b.HistoryContent(r.Context(), r.PathValue("id"), part)
		if err != nil {
			fail(w, 500, err)
			return
		}
		defer func() { _ = c.Close() }()
		ctype := c.MIME
		if ctype == "" {
			ctype = "application/octet-stream"
		}
		w.Header().Set("Content-Type", ctype)
		w.Header().Set("Content-Length", strconv.FormatInt(c.Size, 10))
		if c.Name != "" {
			w.Header().Set("Content-Disposition", mime.FormatMediaType("attachment", map[string]string{"filename": c.Name}))
		}
		_, _ = io.Copy(w, c)
	})
	mux.HandleFunc("POST /v1/history/{id}/use", func(w http.ResponseWriter, r *http.Request) {
		request := r.Header.Get("Idempotency-Key")
		if r.ContentLength != 0 {
			var body struct {
				RequestID string `json:"request_id"`
			}
			dec := json.NewDecoder(io.LimitReader(r.Body, 4096))
			dec.DisallowUnknownFields()
			if err := dec.Decode(&body); err != nil {
				fail(w, 400, errors.New("invalid restore request"))
				return
			}
			if body.RequestID != "" {
				request = body.RequestID
			}
		}
		if request != "" {
			if restorer, ok := b.(interface {
				RestoreHistory(context.Context, string, string) (engine.RestoreSyncResult, error)
			}); ok {
				result, err := restorer.RestoreHistory(r.Context(), r.PathValue("id"), request)
				if err != nil {
					fail(w, 500, err)
					return
				}
				reply(w, 200, result)
				return
			}
		}
		if err := b.UseHistory(r.Context(), r.PathValue("id")); err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})
	mux.HandleFunc("GET /v1/actions", func(w http.ResponseWriter, r *http.Request) {
		list, err := b.Actions(r.Context())
		if err != nil {
			fail(w, 500, err)
			return
		}
		if list == nil {
			list = []api.Action{}
		}
		reply(w, 200, map[string]any{"actions": list})
	})
	mux.HandleFunc("POST /v1/history/{id}/actions/{key...}", func(w http.ResponseWriter, r *http.Request) {
		if h, ok := b.(api.HeadlessBackend); ok {
			var in api.ActionInput
			if r.ContentLength != 0 {
				if e := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096)).Decode(&in); e != nil {
					fail(w, 400, &api.Error{Message: "invalid action request", Reason: "invalid_argument"})
					return
				}
			}
			out, e := h.ExecuteAction(r.Context(), r.PathValue("id"), r.PathValue("key"), in)
			if e != nil {
				fail(w, 500, e)
				return
			}
			reply(w, 200, out)
			return
		}
		it, err := b.RunAction(r.Context(), r.PathValue("id"), r.PathValue("key"))
		if err != nil {
			if it.ID != "" { // the record exists, only the clipboard write failed
				reply(w, 502, map[string]any{"error": err.Error(), "reason": api.ReasonOf(err), "item": it})
				return
			}
			fail(w, 500, err)
			return
		}
		reply(w, 200, map[string]any{"item": it})
	})
	mux.HandleFunc("PATCH /v1/history/{id}", func(w http.ResponseWriter, r *http.Request) {
		var u api.HistoryUpdate
		if err := json.NewDecoder(io.LimitReader(r.Body, 1<<20)).Decode(&u); err != nil {
			fail(w, 400, err)
			return
		}
		it, err := b.UpdateHistory(r.Context(), r.PathValue("id"), u)
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, it)
	})
	mux.HandleFunc("DELETE /v1/history/{id}", func(w http.ResponseWriter, r *http.Request) {
		rev, err := strconv.ParseUint(r.URL.Query().Get("revision"), 10, 64)
		if err != nil {
			fail(w, 400, errors.New("revision: required"))
			return
		}
		if err := b.DeleteHistory(r.Context(), r.PathValue("id"), rev); err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})

	// Server-sent events: one JSON api.Event per "data:" line.
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
	reads := boundedContent(mux, 4)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && strings.HasSuffix(r.URL.Path, "/content") && strings.HasPrefix(r.URL.Path, "/v1/") {
			reads.ServeHTTP(w, r)
		} else {
			mux.ServeHTTP(w, r)
		}
	})
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

func reply(w http.ResponseWriter, code int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(v)
}
func fail(w http.ResponseWriter, def int, err error) {
	code := def
	var ae *api.Error
	if errors.As(err, &ae) && ae.Status != 0 {
		code = ae.Status
	}
	reply(w, code, api.ErrorBody{Error: err.Error(), Reason: api.ReasonOf(err)})
}

func boundedContent(next http.Handler, max int) http.Handler {
	gate := make(chan struct{}, max)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		select {
		case gate <- struct{}{}:
			defer func() { <-gate }()
		default:
			fail(w, 429, &api.Error{Message: "too many content readers", Reason: "resource_exhausted"})
			return
		}
		ctx, cancel := context.WithTimeout(r.Context(), 8*time.Second)
		defer cancel()
		_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(8 * time.Second))
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}
