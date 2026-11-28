package server

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/observability"
)

type diagnosticBackend interface {
	Diagnostics() *observability.Controller
}

func diagnosticRoutes(mux *http.ServeMux, b api.Backend) {
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
		reply(w, status, api.ErrorBody{Error: "invalid diagnostics request", Reason: reason})
	}
	mux.HandleFunc("GET /v1/diagnostics/log-level", func(w http.ResponseWriter, _ *http.Request) { reply(w, 200, controller.Levels()) })
	mux.HandleFunc("GET /v1/diagnostics/telemetry", func(w http.ResponseWriter, _ *http.Request) { reply(w, 200, controller.Telemetry()) })
	mux.HandleFunc("POST /v1/diagnostics/resource-sample", func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(io.LimitReader(r.Body, 1025))
		if err != nil || len(body) == 0 || len(body) > 1024 {
			fail(w, 400, "invalid_argument")
			return
		}
		sample, err := decodeResourceSample(body)
		if err != nil {
			fail(w, 400, "invalid_argument")
			return
		}
		if err := controller.RecordDesktopResourceSample(sample); err != nil {
			status := 400
			reason := "invalid_argument"
			if err.Error() == "unavailable" {
				status = 503
				reason = "unavailable"
			}
			fail(w, status, reason)
			return
		}
		w.WriteHeader(http.StatusNoContent)
	})
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

func decodeResourceSample(b []byte) (observability.ResourceSample, error) {
	dec := json.NewDecoder(bytes.NewReader(b))
	t, err := dec.Token()
	if err != nil || t != json.Delim('{') {
		return observability.ResourceSample{}, errors.New("object required")
	}
	seen := map[string]bool{}
	var s observability.ResourceSample
	for dec.More() {
		t, err := dec.Token()
		if err != nil {
			return observability.ResourceSample{}, err
		}
		k, ok := t.(string)
		if !ok || seen[k] {
			return observability.ResourceSample{}, errors.New("invalid or duplicate field")
		}
		seen[k] = true
		var raw json.RawMessage
		if err := dec.Decode(&raw); err != nil {
			return observability.ResourceSample{}, err
		}
		switch k {
		case "process_role":
			if err := json.Unmarshal(raw, &s.ProcessRole); err != nil {
				return observability.ResourceSample{}, err
			}
		case "app_state":
			if err := json.Unmarshal(raw, &s.AppState); err != nil {
				return observability.ResourceSample{}, err
			}
		case "power_source":
			if err := json.Unmarshal(raw, &s.PowerSource); err != nil {
				return observability.ResourceSample{}, err
			}
		case "window_seconds":
			if err := json.Unmarshal(raw, &s.WindowSeconds); err != nil {
				return observability.ResourceSample{}, err
			}
		case "cpu_seconds":
			if err := json.Unmarshal(raw, &s.CPUSeconds); err != nil {
				return observability.ResourceSample{}, err
			}
		case "cpu_probe_status":
			if err := json.Unmarshal(raw, &s.CPUProbeStatus); err != nil {
				return observability.ResourceSample{}, err
			}
		case "sleep_jumped":
			if err := json.Unmarshal(raw, &s.SleepJumped); err != nil {
				return observability.ResourceSample{}, err
			}
		case "memory_footprint_bytes":
			if err := json.Unmarshal(raw, &s.MemoryFootprintBytes); err != nil {
				return observability.ResourceSample{}, err
			}
		case "memory_probe_status":
			if err := json.Unmarshal(raw, &s.MemoryProbeStatus); err != nil {
				return observability.ResourceSample{}, err
			}
		case "go_heap_bytes":
			if err := json.Unmarshal(raw, &s.GoHeapBytes); err != nil {
				return observability.ResourceSample{}, err
			}
		case "wakeups":
			if err := json.Unmarshal(raw, &s.Wakeups); err != nil {
				return observability.ResourceSample{}, err
			}
		case "wakeups_probe_status":
			if err := json.Unmarshal(raw, &s.WakeupsProbeStatus); err != nil {
				return observability.ResourceSample{}, err
			}
		case "context_switches":
			if err := json.Unmarshal(raw, &s.ContextSwitches); err != nil {
				return observability.ResourceSample{}, err
			}
		case "context_switches_probe_status":
			if err := json.Unmarshal(raw, &s.ContextSwitchesStatus); err != nil {
				return observability.ResourceSample{}, err
			}
		default:
			return observability.ResourceSample{}, errors.New("unknown field")
		}
	}
	if _, err := dec.Token(); err != nil {
		return observability.ResourceSample{}, err
	}
	if _, err := dec.Token(); err != io.EOF {
		return observability.ResourceSample{}, errors.New("trailing input")
	}
	if !seen["process_role"] || !seen["app_state"] || !seen["window_seconds"] || !seen["cpu_seconds"] {
		return observability.ResourceSample{}, errors.New("missing required field")
	}
	return s, nil
}
