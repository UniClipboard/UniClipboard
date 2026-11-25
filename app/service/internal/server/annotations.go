package server

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func annotationRoutes(mux *http.ServeMux, b api.Backend) {
	ab, ok := b.(api.HistoryAnnotationBackend)
	if !ok {
		return
	}
	mux.Handle("GET /v1/history/{id}/annotations", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		anns, err := ab.HistoryAnnotations(r.Context(), r.PathValue("id"))
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, anns)
	}), 4))
	mux.Handle("GET /v1/history/{id}/annotations/{plugin}/{type}/assets/{key}", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		asset, err := ab.HistoryAnnotationAsset(
			r.Context(),
			r.PathValue("id"),
			r.PathValue("plugin"),
			r.PathValue("type"),
			r.PathValue("key"),
		)
		if err != nil {
			fail(w, 500, err)
			return
		}
		defer func() { _ = asset.Close() }()
		if asset.MIME != "" {
			w.Header().Set("Content-Type", asset.MIME)
		}
		if asset.Size >= 0 {
			w.Header().Set("Content-Length", strconv.FormatInt(asset.Size, 10))
		}
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.WriteHeader(http.StatusOK)
		_, _ = io.Copy(w, asset)
	}), 4))
	mux.Handle("POST /v1/history/{id}/enrich", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var req api.HistoryEnrichInput
		if r.Body != nil {
			dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4<<10))
			dec.DisallowUnknownFields()
			if err := dec.Decode(&req); err != nil && !errors.Is(err, io.EOF) {
				fail(w, 400, &api.Error{Message: "invalid enrich request", Reason: "invalid_argument"})
				return
			}
		}
		if err := ab.EnrichHistory(r.Context(), r.PathValue("id"), req); err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, map[string]bool{"queued": true})
	}), 4))
}
