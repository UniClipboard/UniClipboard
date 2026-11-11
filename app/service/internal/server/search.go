package server

import (
	"encoding/json"
	"io"
	"net/http"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func searchRoutes(mux *http.ServeMux, b api.Backend) {
	search, ok := b.(api.HistorySearchBackend)
	if !ok {
		return
	}
	decode := func(w http.ResponseWriter, r *http.Request, v any) bool {
		d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1<<20))
		d.DisallowUnknownFields()
		if err := d.Decode(v); err != nil {
			fail(w, 400, &api.Error{Message: "invalid history search request", Reason: "invalid_argument"})
			return false
		}
		if err := d.Decode(new(any)); err != io.EOF {
			fail(w, 400, &api.Error{Message: "invalid history search request", Reason: "invalid_argument"})
			return false
		}
		return true
	}
	mux.Handle("POST /v1/history/search", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var q engine.HistorySearchRequest
		if !decode(w, r, &q) {
			return
		}
		v, e := search.SearchHistory(r.Context(), q)
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, v)
	}), 4))
	mux.Handle("POST /v1/history/search/candidates", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var q engine.HistorySearchCandidatesRequest
		if !decode(w, r, &q) {
			return
		}
		v, e := search.ListHistorySearchCandidates(r.Context(), q)
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, v)
	}), 4))
	mux.Handle("POST /v1/history/search/counts", boundedRequests(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var q engine.HistorySearchCountsRequest
		if !decode(w, r, &q) {
			return
		}
		v, e := search.CountHistorySearches(r.Context(), q)
		if e != nil {
			fail(w, 500, e)
			return
		}
		reply(w, 200, v)
	}), 4))
}
