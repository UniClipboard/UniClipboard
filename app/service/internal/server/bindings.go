package server

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strconv"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func bindingRoutes(mux *http.ServeMux, b api.Backend) {
	backend, ok := b.(interface {
		SetBindingSync(context.Context, string, api.BindingEdit) (engine.DeviceBinding, error)
		RemoveBinding(context.Context, string, int64) error
	})
	if !ok {
		return
	}
	mux.HandleFunc("PATCH /v1/bindings/{id}", func(w http.ResponseWriter, r *http.Request) {
		var in struct {
			Revision int64 `json:"revision"`
			Enabled  *bool `json:"enabled"`
		}
		dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096))
		dec.DisallowUnknownFields()
		if dec.Decode(&in) != nil || dec.Decode(new(any)) != io.EOF || in.Revision <= 0 || in.Enabled == nil {
			fail(w, 400, &api.Error{Reason: "invalid_argument", Message: "binding revision is required"})
			return
		}
		out, err := backend.SetBindingSync(r.Context(), r.PathValue("id"), api.BindingEdit{Revision: in.Revision, Enabled: *in.Enabled})
		if err != nil {
			fail(w, 400, err)
			return
		}
		reply(w, 200, out)
	})
	mux.HandleFunc("DELETE /v1/bindings/{id}", func(w http.ResponseWriter, r *http.Request) {
		revision, err := strconv.ParseInt(r.URL.Query().Get("revision"), 10, 64)
		if err != nil || revision <= 0 {
			fail(w, 400, &api.Error{Reason: "invalid_argument", Message: "binding revision is required"})
			return
		}
		if err := backend.RemoveBinding(r.Context(), r.PathValue("id"), revision); err != nil {
			fail(w, 400, err)
			return
		}
		reply(w, 200, map[string]bool{"ok": true})
	})
}
