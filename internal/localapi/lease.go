package api

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

// Lifecycle errors a client can branch on.
var (
	// ErrRetiring means the generation closed admission. Retry acquisition before any business request.
	ErrRetiring = errors.New("the unic service is shutting down")
	// ErrGenerationChanged means the observed generation was replaced.
	ErrGenerationChanged = errors.New("the unic service was replaced")
)

// Lease is one live usage of a service generation. The open connection is the lease:
// if the process dies the kernel closes it and the service reclaims the usage.
type Lease struct {
	c          *Client
	generation string
	id         string
	body       io.ReadCloser
	cancel     context.CancelFunc
}

// ID is local control material. Never log it or put it in telemetry.
func (l *Lease) ID() string { return l.id }

// AcquireLease admits one usage of exactly the given generation.
// The stream outlives ctx cancellation of the admission request only through Release or Close.
func (c *Client) AcquireLease(ctx context.Context, generation string) (*Lease, error) {
	body, _ := json.Marshal(map[string]string{"id": generation})
	streamCtx, cancel := context.WithCancel(context.Background())
	// Admission must still obey the caller's deadline.
	stop := context.AfterFunc(ctx, cancel)
	req, err := http.NewRequestWithContext(streamCtx, http.MethodPost, "http://unic/v1/service/leases", bytes.NewReader(body))
	if err != nil {
		stop()
		cancel()
		return nil, err
	}
	hc := &http.Client{Transport: &http.Transport{
		DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
			var d net.Dialer
			return d.DialContext(ctx, "unix", c.Path)
		},
		DisableKeepAlives: true,
	}}
	resp, err := hc.Do(req)
	if err != nil {
		stop()
		cancel()
		if isNoDaemon(err) {
			return nil, ErrNoDaemon
		}
		return nil, err
	}
	if resp.StatusCode != http.StatusOK {
		defer func() { _ = resp.Body.Close() }()
		defer cancel()
		stop()
		var e errBody
		_ = json.NewDecoder(io.LimitReader(resp.Body, 4096)).Decode(&e)
		switch e.Reason {
		case servicelifecycle.ReasonRetiring:
			return nil, ErrRetiring
		case servicelifecycle.ReasonGenerationChanged:
			return nil, ErrGenerationChanged
		}
		if e.Error == "" {
			e.Error = resp.Status
		}
		return nil, &Error{Message: e.Error, Reason: e.Reason}
	}
	var ack struct {
		Lease string `json:"lease"`
	}
	if err := json.NewDecoder(bufio.NewReader(resp.Body)).Decode(&ack); err != nil || ack.Lease == "" {
		_ = resp.Body.Close()
		stop()
		cancel()
		if err == nil {
			err = errors.New("service did not acknowledge usage")
		}
		return nil, err
	}
	// After the acknowledgement only Release or Close ends the stream.
	stop()
	return &Lease{c: c, generation: generation, id: ack.Lease, body: resp.Body, cancel: cancel}, nil
}

// Release ends the usage and reports whether it retired a temporary generation.
// The connection is closed in every case.
func (l *Lease) Release(ctx context.Context) (last bool, err error) {
	defer l.Close()
	var out struct {
		Last bool `json:"last"`
	}
	err = l.c.do(ctx, http.MethodDelete, "/v1/service/leases/"+l.id, map[string]string{"id": l.generation}, &out)
	return out.Last, err
}

// Close drops the connection. The service then releases the lease as a disconnect.
func (l *Lease) Close() {
	l.cancel()
	_ = l.body.Close()
}

// Operation returns the current state of an Engine operation started by this service generation.
func (c *Client) Operation(ctx context.Context, id string) (out engine.Operation, err error) {
	err = c.do(ctx, http.MethodGet, "/v1/operations/"+id, nil, &out)
	return
}
