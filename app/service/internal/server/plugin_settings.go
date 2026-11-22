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
	mux.HandleFunc("GET /v1/plugins/marketplace", func(w http.ResponseWriter, r *http.Request) {
		mb, ok := backend.(interface {
			PluginMarketplace(context.Context, bool) (api.PluginMarketplace, error)
		})
		if !ok {
			fail(w, 400, &api.Error{Reason: "unsupported", Message: "plugin marketplace unavailable"})
			return
		}
		refresh := r.URL.Query().Get("refresh") == "1" || r.URL.Query().Get("refresh") == "true"
		out, err := mb.PluginMarketplace(r.Context(), refresh)
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, out)
	})
	mux.HandleFunc("POST /v1/plugins/install", func(w http.ResponseWriter, r *http.Request) {
		ib, ok := backend.(interface {
			InstallPlugin(context.Context, api.PluginInstallRequest) (api.PluginInfo, error)
		})
		if !ok {
			fail(w, 400, &api.Error{Reason: "unsupported", Message: "plugin installation unavailable"})
			return
		}
		var in api.PluginInstallRequest
		dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 8192))
		dec.DisallowUnknownFields()
		if dec.Decode(&in) != nil || dec.Decode(new(any)) != io.EOF || in.Repository == "" {
			fail(w, 400, &api.Error{Reason: "invalid_argument", Message: "invalid plugin install request"})
			return
		}
		out, err := ib.InstallPlugin(r.Context(), in)
		if err != nil {
			fail(w, 400, err)
			return
		}
		reply(w, 200, out)
	})
	mux.HandleFunc("DELETE /v1/plugins/{id}", func(w http.ResponseWriter, r *http.Request) {
		ub, ok := backend.(interface {
			UninstallPlugin(context.Context, string) error
		})
		if !ok {
			fail(w, 400, &api.Error{Reason: "unsupported", Message: "plugin uninstall unavailable"})
			return
		}
		if err := ub.UninstallPlugin(r.Context(), r.PathValue("id")); err != nil {
			fail(w, 400, err)
			return
		}
		reply(w, 200, map[string]any{"ok": true})
	})
	mux.HandleFunc("POST /v1/plugins/{id}/rollback", func(w http.ResponseWriter, r *http.Request) {
		rb, ok := backend.(interface {
			RollbackPlugin(context.Context, string) (api.PluginInfo, error)
		})
		if !ok {
			fail(w, 400, &api.Error{Reason: "unsupported", Message: "plugin rollback unavailable"})
			return
		}
		out, err := rb.RollbackPlugin(r.Context(), r.PathValue("id"))
		if err != nil {
			fail(w, 400, err)
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
