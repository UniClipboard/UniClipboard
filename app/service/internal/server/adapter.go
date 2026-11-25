package server

import (
	"context"
	"encoding/json"
	"io"
	"net/http"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func adapterHandler(b api.Backend) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		backend, ok := b.(interface {
			AdapterCall(context.Context, api.AdapterRequest) api.AdapterReply
		})
		if !ok {
			fail(w, 501, &api.Error{Message: "adapter sessions unsupported", Reason: "unsupported"})
			return
		}
		var in api.AdapterRequest
		dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 96<<20))
		dec.DisallowUnknownFields()
		if err := dec.Decode(&in); err != nil {
			fail(w, 400, &api.Error{Message: "invalid adapter request", Reason: "invalid_argument"})
			return
		}
		reply(w, 200, backend.AdapterCall(r.Context(), in))
	}
}

func panelApplyRoutes(mux *http.ServeMux, b api.Backend) {
	mux.Handle("POST /v1/desktop/panel-apply/begin", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		backend, ok := b.(interface {
			PanelApplyBegin(context.Context, api.PanelApplyBeginRequest) (api.PanelApplyGrant, error)
		})
		if !ok {
			fail(w, 501, &api.Error{Message: "panel apply unsupported", Reason: "unsupported"})
			return
		}
		var in api.PanelApplyBeginRequest
		dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 96<<20))
		dec.DisallowUnknownFields()
		if err := dec.Decode(&in); err != nil || dec.Decode(new(any)) != io.EOF {
			fail(w, 400, &api.Error{Message: "invalid panel apply begin request", Reason: "invalid_argument"})
			return
		}
		out, err := backend.PanelApplyBegin(r.Context(), in)
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, out)
	}), 8))

	mux.Handle("POST /v1/desktop/panel-apply/complete", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		backend, ok := b.(interface {
			PanelApplyComplete(context.Context, api.PanelApplyCompleteRequest) (api.PanelApplyCompleteReply, error)
		})
		if !ok {
			fail(w, 501, &api.Error{Message: "panel apply unsupported", Reason: "unsupported"})
			return
		}
		var in api.PanelApplyCompleteRequest
		dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 96<<20))
		dec.DisallowUnknownFields()
		if err := dec.Decode(&in); err != nil || dec.Decode(new(any)) != io.EOF {
			fail(w, 400, &api.Error{Message: "invalid panel apply complete request", Reason: "invalid_argument"})
			return
		}
		out, err := backend.PanelApplyComplete(r.Context(), in)
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, out)
	}), 8))

	mux.HandleFunc("POST /v1/desktop/panel-apply/release", func(w http.ResponseWriter, r *http.Request) {
		backend, ok := b.(interface {
			PanelApplyRelease(context.Context, api.PanelApplyReleaseRequest) error
		})
		if !ok {
			fail(w, 501, &api.Error{Message: "panel apply unsupported", Reason: "unsupported"})
			return
		}
		var in api.PanelApplyReleaseRequest
		dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4<<10))
		dec.DisallowUnknownFields()
		if err := dec.Decode(&in); err != nil || dec.Decode(new(any)) != io.EOF {
			fail(w, 400, &api.Error{Message: "invalid panel apply release request", Reason: "invalid_argument"})
			return
		}
		if err := backend.PanelApplyRelease(r.Context(), in); err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})
}
