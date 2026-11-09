package server

import (
	"context"
	"io"
	"net/http"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"

	"github.com/uniclipboard/unic/engine"
)

func adapterContentHandler(b api.Backend) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		v, ok := b.(interface {
			AdapterContent(context.Context, string, engine.LeaseID, engine.PartID) (api.HistoryContent, error)
		})
		if !ok {
			fail(w, 501, &api.Error{Reason: "unsupported", Message: "adapter unavailable"})
			return
		}
		c, err := v.AdapterContent(r.Context(), r.Header.Get("X-Unic-Adapter"), engine.LeaseID(r.URL.Query().Get("lease")), engine.PartID(r.URL.Query().Get("part")))
		if err != nil {
			fail(w, 503, err)
			return
		}
		defer func() { _ = c.Close() }()
		_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(8 * time.Second))
		w.Header().Set("Content-Type", "application/octet-stream")
		_, _ = io.Copy(w, c)
	}
}
