package server

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func channelPairRoute(mux *http.ServeMux, b api.Backend) {
	mux.HandleFunc("POST /v1/channels/pair", func(w http.ResponseWriter, r *http.Request) {
		pairer, ok := b.(interface {
			PairChannel(context.Context, api.ChannelPairRequest) (api.PeerStatus, error)
		})
		if !ok {
			fail(w, 501, errors.New("channel pairing unavailable"))
			return
		}
		var in api.ChannelPairRequest
		dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096))
		dec.DisallowUnknownFields()
		if dec.Decode(&in) != nil || dec.Decode(new(any)) != io.EOF || in.BackendID == "" || in.Resume == (in.Code != "") {
			fail(w, 400, errors.New("a backend and either a pairing code or explicit resume are required"))
			return
		}
		ctx, cancel := context.WithTimeout(r.Context(), 90*time.Second)
		defer cancel()
		out, err := pairer.PairChannel(ctx, in)
		if err != nil {
			fail(w, 500, err)
			return
		}
		reply(w, 200, out)
	})
}
