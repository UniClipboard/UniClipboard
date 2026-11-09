package api

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"

	"github.com/uniclipboard/unic/internal/observability"
)

type diagnosticBackend interface {
	Diagnostics() *observability.Controller
}

func diagnosticRoutes(mux *http.ServeMux, b Backend) {
	backend, ok := b.(diagnosticBackend)
	if !ok {
		return
	}
	controller := backend.Diagnostics()
	reply := func(w http.ResponseWriter, status int, v any) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_ = json.NewEncoder(w).Encode(v)
	}
	fail := func(w http.ResponseWriter, status int, reason string) {
		reply(w, status, errBody{Error: "invalid diagnostics request", Reason: reason})
	}
	mux.HandleFunc("GET /v1/diagnostics/log-level", func(w http.ResponseWriter, _ *http.Request) { reply(w, 200, controller.Levels()) })
	mux.HandleFunc("GET /v1/diagnostics/telemetry", func(w http.ResponseWriter, _ *http.Request) { reply(w, 200, controller.Telemetry()) })
	mux.HandleFunc("PUT /v1/diagnostics/log-level", func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(io.LimitReader(r.Body, 4097))
		if err != nil || len(body) > 4096 {
			fail(w, 400, "invalid_argument")
			return
		}
		// Decode the flat object token by token to reject duplicate keys as well as
		// unknown fields. encoding/json's struct decoder accepts duplicates.
		fields, err := diagnosticFields(body)
		if err != nil {
			fail(w, 400, "invalid_argument")
			return
		}
		var target, level string
		var seconds int
		if json.Unmarshal(fields["target"], &target) != nil || json.Unmarshal(fields["level"], &level) != nil || string(fields["duration_seconds"]) == "null" || json.Unmarshal(fields["duration_seconds"], &seconds) != nil {
			fail(w, 400, "invalid_argument")
			return
		}
		state, err := controller.SetLevel(target, level, seconds, false)
		if err != nil {
			status := 400
			reason := "invalid_argument"
			if err.Error() == "unavailable" {
				status = 503
				reason = "unavailable"
			}
			fail(w, status, reason)
			return
		}
		reply(w, 200, state)
	})
	mux.HandleFunc("DELETE /v1/diagnostics/log-level", func(w http.ResponseWriter, r *http.Request) {
		q, err := parseDiagnosticTarget(r)
		if err != nil {
			fail(w, 400, "invalid_argument")
			return
		}
		state, err := controller.SetLevel(q, "", 0, true)
		if err != nil {
			status := 400
			reason := "invalid_argument"
			if err.Error() == "unavailable" {
				status = 503
				reason = "unavailable"
			}
			fail(w, status, reason)
			return
		}
		reply(w, 200, state)
	})
}
func diagnosticFields(b []byte) (map[string]json.RawMessage, error) {
	dec := json.NewDecoder(bytes.NewReader(b))
	t, err := dec.Token()
	if err != nil || t != json.Delim('{') {
		return nil, errors.New("object required")
	}
	fields := map[string]json.RawMessage{}
	for dec.More() {
		t, err := dec.Token()
		if err != nil {
			return nil, err
		}
		k, ok := t.(string)
		if !ok || (k != "target" && k != "level" && k != "duration_seconds") || fields[k] != nil {
			return nil, errors.New("invalid field")
		}
		var v json.RawMessage
		if err := dec.Decode(&v); err != nil {
			return nil, err
		}
		fields[k] = v
	}
	if _, err := dec.Token(); err != nil {
		return nil, err
	}
	if len(fields) != 3 {
		return nil, errors.New("missing field")
	}
	if _, err := dec.Token(); err != io.EOF {
		return nil, errors.New("trailing input")
	}
	return fields, nil
}
func parseDiagnosticTarget(r *http.Request) (string, error) {
	q, parseErr := url.ParseQuery(r.URL.RawQuery)
	if parseErr != nil {
		return "", errors.New("invalid target")
	}
	targets := q["target"]
	if len(q) != 1 || len(targets) != 1 || (targets[0] != "local" && targets[0] != "remote") {
		return "", errors.New("invalid target")
	}
	return targets[0], nil
}
func (c *Client) LogLevels(ctx context.Context) (v observability.LogLevelState, err error) {
	err = c.do(ctx, "GET", "/v1/diagnostics/log-level", nil, &v)
	return
}
func (c *Client) SetLogLevel(ctx context.Context, target, level string, seconds int) (v observability.LogLevelState, err error) {
	err = c.do(ctx, "PUT", "/v1/diagnostics/log-level", map[string]any{"target": target, "level": level, "duration_seconds": seconds}, &v)
	return
}
func (c *Client) ResetLogLevel(ctx context.Context, target string) (v observability.LogLevelState, err error) {
	err = c.do(ctx, "DELETE", "/v1/diagnostics/log-level?target="+target, nil, &v)
	return
}
func (c *Client) Telemetry(ctx context.Context) (v observability.TelemetryState, err error) {
	err = c.do(ctx, "GET", "/v1/diagnostics/telemetry", nil, &v)
	return
}
