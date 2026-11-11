//go:build unic_e2e

package main

import (
	"net/http"
	"sync/atomic"
	"time"
)

var syncFixtureTransport *e2eTransport
var syncFixtureDay atomic.Int64

func syncFixtureRoutes(mux *http.ServeMux) {
	mux.HandleFunc("POST /test/sync/{operation}", func(w http.ResponseWriter, r *http.Request) {
		if syncFixtureTransport == nil {
			http.Error(w, "missing transport", 500)
			return
		}
		switch r.PathValue("operation") {
		case "path-derp":
			syncFixtureTransport.setPath("derp")
		case "path-direct":
			syncFixtureTransport.setPath("direct")
		case "path-unknown":
			syncFixtureTransport.setPath("unknown")
		case "disconnect":
			syncFixtureTransport.disconnect()
		case "dial-timeout":
			syncFixtureTransport.mu.Lock()
			syncFixtureTransport.dialTimeout = true
			syncFixtureTransport.mu.Unlock()
		case "dial-normal":
			syncFixtureTransport.mu.Lock()
			syncFixtureTransport.dialTimeout = false
			syncFixtureTransport.mu.Unlock()
		case "partial-body":
			syncFixtureTransport.mu.Lock()
			syncFixtureTransport.partial = 17
			syncFixtureTransport.mu.Unlock()
		case "next-day":
			syncFixtureDay.Add(1)
		default:
			http.NotFound(w, r)
			return
		}
		w.WriteHeader(200)
	})
}
func syncFixtureNow() time.Time {
	return time.Now().Add(time.Duration(syncFixtureDay.Load()) * 24 * time.Hour)
}
