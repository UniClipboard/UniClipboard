package server

import (
	"context"
	"encoding/json"
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
