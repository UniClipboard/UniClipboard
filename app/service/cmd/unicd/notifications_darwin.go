package main

import (
	"context"
	"encoding/json"
	"net/http"
	"sync"

	"github.com/uniclipboard/unic/app/service/internal/host"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/pairnotice"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func registerDesktopNotifications(ctx context.Context, mux *http.ServeMux, n *host.Node, settings *config.Manager, lifecycle *servicelifecycle.Service) {
	var subscribe sync.Once
	mux.HandleFunc("POST /v1/desktop/notifications/enable", func(w http.ResponseWriter, r *http.Request) {
		state := lifecycle.Info()
		enabled := ctx.Err() == nil && !state.Retiring && state.Mode != servicelifecycle.ModeCLITemporary && pairnotice.Available()
		if enabled {
			subscribe.Do(func() {
				events, closeEvents := n.Subscribe()
				go func() {
					defer closeEvents()
					for {
						select {
						case <-ctx.Done():
							return
						case event, ok := <-events:
							if !ok {
								return
							}
							if event.Kind == "pending" {
								title, body := pairnotice.Text(settings.Current().Desktop.Language, event.Peer, event.Info)
								_ = pairnotice.Show(ctx, title, body)
							}
						}
					}
				}()
			})
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(enabled)
	})
}
