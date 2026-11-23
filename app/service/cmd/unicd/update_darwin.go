package main

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"runtime"
	"strconv"
	"time"

	"github.com/uniclipboard/unic/internal/desktopupdate"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/nativeboard"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

var desktopUpdateFeed = desktopupdate.FeedBase

func registerDesktopUpdate(mux *http.ServeMux, settings *config.Manager, lifecycle *servicelifecycle.Service) {
	type selection struct {
		token, address string
		size           int64
		source         *desktopupdate.Source
	}
	// The work gate serializes access to the current selection.
	var chosen selection
	work := make(chan struct{}, 1)
	available := func() bool {
		state := lifecycle.Info()
		return !state.Retiring && state.Mode != servicelifecycle.ModeCLITemporary && nativeboard.ActiveConsoleUser()
	}
	acquire := func(w http.ResponseWriter) bool {
		if !available() {
			http.Error(w, "desktop updates are unavailable", http.StatusConflict)
			return false
		}
		select {
		case work <- struct{}{}:
			return true
		default:
			http.Error(w, "desktop update is busy", http.StatusConflict)
			return false
		}
	}
	mux.HandleFunc("POST /v1/desktop/update/check", func(w http.ResponseWriter, r *http.Request) {
		if !acquire(w) {
			return
		}
		defer func() { <-work }()
		var request api.DesktopUpdateRequest
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096)).Decode(&request); err != nil {
			http.Error(w, "invalid update request", http.StatusBadRequest)
			return
		}
		key, err := base64.StdEncoding.DecodeString(request.PublicKey)
		if err != nil || len(key) != ed25519.PublicKeySize || len(request.CurrentVersion) == 0 || len(request.CurrentVersion) > 96 {
			http.Error(w, "invalid update identity", http.StatusBadRequest)
			return
		}
		prefs := settings.Current().Desktop
		source := &desktopupdate.Source{
			Base: desktopUpdateFeed, Target: runtime.GOOS + "-" + runtime.GOARCH,
			Key: request.PublicKey, CurrentVersion: request.CurrentVersion,
			Channel: func() string { return prefs.UpdateChannel }, Language: func() string { return prefs.Language },
		}
		body, err := source.Manifest(r.Context(), false)
		if err != nil {
			http.Error(w, "desktop update check failed", http.StatusBadGateway)
			return
		}
		defer func() { _ = body.Close() }()
		manifest, err := io.ReadAll(body)
		var artifact struct {
			URL  string `json:"url"`
			Size int64  `json:"size"`
		}
		if err != nil || json.Unmarshal(manifest, &artifact) != nil {
			http.Error(w, "invalid desktop update manifest", http.StatusBadGateway)
			return
		}
		token := rand.Text()
		chosen = selection{token: token, address: artifact.URL, size: artifact.Size, source: source}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(api.DesktopUpdateSelection{Token: token, Manifest: manifest})
	})
	mux.HandleFunc("GET /v1/desktop/update/archive/{token}", func(w http.ResponseWriter, r *http.Request) {
		if !acquire(w) {
			return
		}
		defer func() { <-work }()
		current := chosen
		if current.token == "" || current.token != r.PathValue("token") {
			http.Error(w, "desktop update selection expired", http.StatusNotFound)
			return
		}
		archive, err := current.source.Open(r.Context(), current.address)
		if err != nil {
			http.Error(w, "desktop update verification failed", http.StatusBadGateway)
			return
		}
		defer func() { _ = archive.Close() }()
		w.Header().Set("Content-Type", "application/gzip")
		w.Header().Set("Content-Length", strconv.FormatInt(current.size, 10))
		_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(2 * time.Minute))
		_, _ = io.Copy(w, archive)
	})
}
