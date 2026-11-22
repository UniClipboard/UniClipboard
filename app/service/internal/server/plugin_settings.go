package server

import (
	"context"
	"encoding/json"
	"io"
	"net/http"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func pluginRoutes(mux *http.ServeMux, backend api.Backend) {
	b, ok := backend.(interface {
		Plugins(context.Context) ([]api.PluginInfo, error)
		PluginSettings(context.Context, string) (api.PluginSettings, error)
		SavePluginSettings(context.Context, string, api.PluginSettingsEdit) (api.PluginSettings, error)
	})
	if !ok {
		return
	}
	mux.HandleFunc("POST /v1/plugin-instances", func(w http.ResponseWriter, r *http.Request) {
		b, ok := backend.(interface {
			CreatePluginInstance(context.Context, api.PluginInstanceCreate) (api.PluginInfo, error)
		})
		if !ok {
			fail(w, 400, &api.Error{Reason: "unsupported", Message: "instance creation unavailable"})
			return
		}
		var in api.PluginInstanceCreate
		dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096))
		dec.DisallowUnknownFields()
		if dec.Decode(&in) != nil || dec.Decode(new(any)) != io.EOF || in.Revision == "" {
			fail(w, 400, &api.Error{Reason: "invalid_argument", Message: "invalid instance request"})
			return
		}
		out, err := b.CreatePluginInstance(r.Context(), in)
		if err != nil {
			fail(w, 400, err)
			return
		}
		reply(w, 200, out)
	})
	mux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, r *http.Request) {
		out, err := b.Plugins(r.Context())
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, out)
	})
	mux.HandleFunc("GET /v1/plugins/{id}/settings", func(w http.ResponseWriter, r *http.Request) {
		out, err := b.PluginSettings(r.Context(), r.PathValue("id"))
		if err != nil {
			fail(w, 400, err)
			return
		}
		reply(w, 200, out)
	})
	mux.HandleFunc("PUT /v1/plugins/{id}/settings", func(w http.ResponseWriter, r *http.Request) {
		var in api.PluginSettingsEdit
		dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 128<<10))
		dec.DisallowUnknownFields()
		if dec.Decode(&in) != nil || dec.Decode(new(any)) != io.EOF || in.Revision == "" || len(in.Values) == 0 {
			fail(w, 400, &api.Error{Reason: "invalid_argument", Message: "invalid plugin settings request"})
			return
		}
		out, err := b.SavePluginSettings(r.Context(), r.PathValue("id"), in)
		if err != nil {
			fail(w, 400, err)
			return
		}
		reply(w, 200, out)
	})
}
