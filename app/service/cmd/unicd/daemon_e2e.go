//go:build unic_e2e

package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/directory"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/uniclipboard/unic/app/service/internal/host"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/testboard"
)

// retireHold keeps the listener open after shutdown starts so a test can meet a retiring generation.
func retireHold() {
	if ms, err := strconv.Atoi(os.Getenv("UNIC_E2E_RETIRE_HOLD_MS")); err == nil && ms > 0 {
		time.Sleep(time.Duration(ms) * time.Millisecond)
	}
}

func configureDaemon(o *host.Options) error {
	// A slow startup lets a test kill the launching CLI before readiness.
	if ms, err := strconv.Atoi(os.Getenv("UNIC_E2E_START_DELAY_MS")); err == nil && ms > 0 {
		time.Sleep(time.Duration(ms) * time.Millisecond)
	}
	if os.Getenv("UNIC_E2E_POSTHOG") == "true" {
		o.Observability.Token = func(context.Context) (string, error) { return "phc_unic_e2e_only", nil }
		o.Observability.AnalyticsInterval = 100 * time.Millisecond
	}
	if os.Getenv("UNIC_E2E_FAST_DIAGNOSTICS") == "1" {
		o.Observability.MetricsInterval = 500 * time.Millisecond
		o.Observability.LogInterval = 100 * time.Millisecond
	}
	if os.Getenv("UNIC_E2E_DIAGNOSTICS") == "1" {
		tr, e := newE2ETransport()
		if e != nil {
			return e
		}
		o.Transport = tr
		syncFixtureTransport = tr
		o.Observability.Now = syncFixtureNow
	}
	return nil
}
func daemonTestRoutes(ctx context.Context, mux *http.ServeMux, n *host.Node, sock string) {
	if os.Getenv("UNIC_E2E_DIAGNOSTICS") == "1" {
		testDiagnosticRoutes(mux, n)
		syncFixtureRoutes(mux)
	}
	if d := os.Getenv("UNIC_MOCK_CLIPBOARD_DIR"); d != "" {
		directoryTestRoutes(mux, n)
		go func() { _ = clipboardintegration.RunRemote(ctx, api.NewClient(sock), testboard.Dir(d), nil) }()
	}
}

func directoryTestRoutes(mux *http.ServeMux, n *host.Node) {
	mux.HandleFunc("POST /test/directory-attack", func(w http.ResponseWriter, r *http.Request) {
		var in struct{ Mode string }
		if json.NewDecoder(r.Body).Decode(&in) != nil {
			w.WriteHeader(400)
			return
		}
		sum := sha256.Sum256([]byte("x"))
		m := directory.Manifest{Roots: []directory.Root{{Name: "attack-root", Directory: true}}, Entries: []directory.Entry{{Root: 0, Directory: true}, {Root: 0, Path: "member", File: 0, Size: 1, SHA256: hex.EncodeToString(sum[:])}}}
		switch in.Mode {
		case "traversal":
			m.Entries[1].Path = "../escape"
		case "absolute":
			m.Entries[1].Path = "/escape"
		case "duplicate":
			m.Entries = append(m.Entries, m.Entries[1])
		case "orphan":
			m.Entries[1].File = 2
		case "hash":
			m.Entries[1].SHA256 = strings.Repeat("0", 64)
		case "size":
			m.Entries[1].Size = 2
		case "reserved":
			m.Entries[1].Path = "CON"
		case "manifest-limit":
		default:
			w.WriteHeader(400)
			return
		}
		data, _ := json.Marshal(m)
		if in.Mode == "manifest-limit" {
			data = append(data, []byte(strings.Repeat(" ", directory.MaxManifest))...)
		}
		h, err := n.Engine().ImportContent(r.Context(), engine.ContentInput{Representations: []engine.RepresentationInput{{MIME: directory.MIME, Size: int64(len(data)), Reader: strings.NewReader(string(data))}}, Files: []engine.FileInput{{Name: "member", Size: 1, Reader: strings.NewReader("x")}}})
		if err != nil {
			w.WriteHeader(500)
			return
		}
		defer n.Engine().ReleaseContent(context.Background(), h.LeaseID)
		var out engine.SubmissionResult
		for attempt := 0; attempt < 30; attempt++ {
			out, err = n.Engine().CommitContent(r.Context(), engine.CommitRequest{RequestID: engine.RequestID("attack-" + in.Mode + "-" + strconv.Itoa(attempt)), LeaseID: h.LeaseID})
			if err != nil || out.SyncDisposition == engine.SyncQueued {
				break
			}
			time.Sleep(200 * time.Millisecond)
		}
		if out.SyncDisposition != engine.SyncQueued {
			w.WriteHeader(500)
			return
		}
		if err != nil {
			w.WriteHeader(500)
			return
		}
		_ = json.NewEncoder(w).Encode(out)
	})

	mux.HandleFunc("GET /test/directories", func(w http.ResponseWriter, r *http.Request) {
		scans, snapshots := n.Engine().DirectoryProbeCounts()
		_ = json.NewEncoder(w).Encode(map[string]uint64{"scans": scans, "snapshots": snapshots})
	})
	mux.HandleFunc("PUT /test/directories", func(w http.ResponseWriter, r *http.Request) {
		var in struct {
			Hold  bool
			Stage string
		}
		if json.NewDecoder(r.Body).Decode(&in) != nil {
			w.WriteHeader(400)
			return
		}
		if in.Stage == "" {
			in.Stage = "scan"
		}
		n.Engine().HoldDirectoryStage(in.Hold, in.Stage)
		w.WriteHeader(200)
	})
}
