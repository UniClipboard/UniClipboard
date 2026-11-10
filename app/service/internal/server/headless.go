package server

import (
	"encoding/json"
	"io"
	"net/http"
	"strconv"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func headlessRoutes(mux *http.ServeMux, b api.Backend) {
	h, ok := b.(api.HeadlessBackend)
	if !ok {
		return
	}
	mux.HandleFunc("GET /v1/capabilities", func(w http.ResponseWriter, r *http.Request) {
		v, e := h.Capabilities(r.Context())
		if e != nil {
			fail(w, 503, e)
			return
		}
		reply(w, 200, v)
	})
	mux.Handle("POST /v1/submissions", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var v api.SubmitInput
		d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 96<<20))
		d.DisallowUnknownFields()
		if e := d.Decode(&v); e != nil {
			fail(w, 400, &api.Error{Message: "invalid submission", Reason: "invalid_argument"})
			return
		}
		out, e := h.Submit(r.Context(), v)
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, out)
	}), 8))
	mux.HandleFunc("GET /v1/submissions/{id}", func(w http.ResponseWriter, r *http.Request) {
		v, e := h.Submission(r.Context(), r.PathValue("id"))
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, v)
	})
	mux.HandleFunc("GET /v1/operations/{id}", func(w http.ResponseWriter, r *http.Request) {
		v, e := h.Operation(r.Context(), r.PathValue("id"))
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, v)
	})
	mux.HandleFunc("GET /v1/received", func(w http.ResponseWriter, r *http.Request) {
		v, e := h.Received(r.Context())
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, v)
	})
	mux.HandleFunc("DELETE /v1/received/{id}", func(w http.ResponseWriter, r *http.Request) {
		if e := h.DismissReceived(r.Context(), r.PathValue("id")); e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})
	mux.HandleFunc("GET /v1/received/{id}/content", func(w http.ResponseWriter, r *http.Request) {
		part := 0
		var e error
		if v := r.URL.Query().Get("part"); v != "" {
			part, e = strconv.Atoi(v)
		}
		if e != nil || part < 0 {
			fail(w, 400, &api.Error{Message: "invalid part", Reason: "invalid_argument"})
			return
		}
		v, e := h.ReceivedContent(r.Context(), r.PathValue("id"), part)
		if e != nil {
			fail(w, 500, e)
			return
		}
		defer func() { _ = v.Close() }()
		w.Header().Set("Content-Type", v.MIME)
		_, _ = io.Copy(w, v)
	})
}

func transferRoutes(mux *http.ServeMux, b api.Backend) {
	t, ok := b.(api.TransferBackend)
	if !ok {
		return
	}
	mux.Handle("POST /v1/transfers", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var v api.StartTransferInput
		d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 96<<20))
		d.DisallowUnknownFields()
		if e := d.Decode(&v); e != nil {
			fail(w, 400, &api.Error{Message: "invalid transfer", Reason: "invalid_argument"})
			return
		}
		out, e := t.StartTransfer(r.Context(), v)
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, out)
	}), 4))
	mux.HandleFunc("GET /v1/transfers/{id}", func(w http.ResponseWriter, r *http.Request) {
		out, e := t.Transfer(r.Context(), r.PathValue("id"))
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, out)
	})
	mux.HandleFunc("DELETE /v1/transfers/{id}", func(w http.ResponseWriter, r *http.Request) {
		if e := t.CancelTransfer(r.Context(), r.PathValue("id")); e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})
	mux.Handle("POST /v1/receive", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var v api.ReceiveInput
		d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 8<<10))
		d.DisallowUnknownFields()
		if e := d.Decode(&v); e != nil {
			fail(w, 400, &api.Error{Message: "invalid receive request", Reason: "invalid_argument"})
			return
		}
		out, e := t.Receive(r.Context(), v)
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, out)
	}), 4))
}

func boundedRequests(next http.Handler, max int) http.Handler {
	gate := make(chan struct{}, max)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		select {
		case gate <- struct{}{}:
			defer func() { <-gate }()
			next.ServeHTTP(w, r)
		default:
			fail(w, 429, &api.Error{Message: "too many local requests", Reason: "resource_exhausted"})
		}
	})
}
