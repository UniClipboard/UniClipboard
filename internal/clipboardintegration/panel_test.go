package clipboardintegration_test

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type mockPanelBoard struct {
	mu          sync.Mutex
	snap        clip.Snapshot
	readErr     error
	writeErr    error
	tamperAfter bool
	onRelease   func()
	watchers    []func()
	writes      int
}

func (b *mockPanelBoard) Read() (clip.Snapshot, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.readErr != nil {
		return clip.Snapshot{}, b.readErr
	}
	return b.snap, nil
}

func (b *mockPanelBoard) Write(s clip.Snapshot) error {
	return b.WriteWithRelease(s, nil)
}

func (b *mockPanelBoard) WriteWithRelease(s clip.Snapshot, onRelease func()) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.writes++
	if b.writeErr != nil {
		return b.writeErr
	}
	b.snap = s
	if b.tamperAfter {
		b.snap = clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("tampered")}}}
	}
	b.onRelease = onRelease
	return nil
}

func (b *mockPanelBoard) WatchChanges(fn func()) func() {
	b.mu.Lock()
	b.watchers = append(b.watchers, fn)
	idx := len(b.watchers) - 1
	b.mu.Unlock()
	return func() {
		b.mu.Lock()
		b.watchers[idx] = nil
		b.mu.Unlock()
	}
}

func (b *mockPanelBoard) triggerRelease() {
	b.mu.Lock()
	fn := b.onRelease
	b.onRelease = nil
	b.mu.Unlock()
	if fn != nil {
		fn()
	}
}

func (b *mockPanelBoard) setExternal(s clip.Snapshot) {
	b.mu.Lock()
	b.snap = s
	ws := append([]func(){}, b.watchers...)
	b.mu.Unlock()
	for _, fn := range ws {
		if fn != nil {
			fn()
		}
	}
}

func TestPanelClipboardProtocolAndFileLease(t *testing.T) {
	dir, err := os.MkdirTemp("/tmp", "uc-pc-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	sock := filepath.Join(dir, "p.sock")
	ln, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	var mu sync.Mutex
	var completes []api.PanelApplyCompleteRequest
	var releases []api.PanelApplyReleaseRequest
	var nextGrant api.PanelApplyGrant
	var failComplete bool
	var beginErrStatus int
	var beginErrReason string

	mux := http.NewServeMux()
	mux.HandleFunc("POST /v1/desktop/panel-apply/begin", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		st, reason, grant := beginErrStatus, beginErrReason, nextGrant
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		if st != 0 {
			w.WriteHeader(st)
			_ = json.NewEncoder(w).Encode(api.ErrorBody{Error: reason, Reason: reason})
			return
		}
		_ = json.NewEncoder(w).Encode(grant)
	})
	mux.HandleFunc("POST /v1/desktop/panel-apply/complete", func(w http.ResponseWriter, r *http.Request) {
		var req api.PanelApplyCompleteRequest
		_ = json.NewDecoder(r.Body).Decode(&req)
		mu.Lock()
		completes = append(completes, req)
		fc := failComplete
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		if fc {
			w.WriteHeader(http.StatusConflict)
			_ = json.NewEncoder(w).Encode(api.ErrorBody{Error: "generation changed", Reason: "generation_changed"})
			return
		}
		_ = json.NewEncoder(w).Encode(api.PanelApplyCompleteReply{Applied: req.Outcome == "applied"})
	})
	mux.HandleFunc("POST /v1/desktop/panel-apply/release", func(w http.ResponseWriter, r *http.Request) {
		var req api.PanelApplyReleaseRequest
		_ = json.NewDecoder(r.Body).Decode(&req)
		mu.Lock()
		releases = append(releases, req)
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]bool{"ok": true})
	})

	srv := &http.Server{Handler: mux}
	go func() { _ = srv.Serve(ln) }()
	defer func() { _ = srv.Close() }()

	client := api.NewClient(sock)
	board := &mockPanelBoard{}
	pc := clipboardintegration.NewPanelClipboard(dir, board)
	defer pc.Close()

	// 1. Plain error mapping.
	mu.Lock()
	beginErrStatus, beginErrReason = http.StatusBadRequest, "plain"
	mu.Unlock()
	if _, err := pc.Apply(context.Background(), client, "h1", true); !errors.Is(err, clipboardintegration.ErrPlain) {
		t.Fatalf("expected ErrPlain, got %v", err)
	}
	mu.Lock()
	beginErrStatus, beginErrReason = 0, ""
	mu.Unlock()

	// 2. Read-back mismatch reports failed completion and returns ErrClipboard.
	sampleFile := filepath.Join(dir, "doc.txt")
	if err := os.WriteFile(sampleFile, []byte("hello"), 0600); err != nil {
		t.Fatal(err)
	}
	mu.Lock()
	nextGrant = api.PanelApplyGrant{
		ServiceID: "svc-1",
		Epoch:     "ep-1",
		Token:     engine.ApplyToken("tok-1"),
		Snapshot:  clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("expected")}}},
	}
	mu.Unlock()
	board.tamperAfter = true
	if _, err := pc.Apply(context.Background(), client, "h1", false); !errors.Is(err, clipboardintegration.ErrClipboard) {
		t.Fatalf("expected ErrClipboard on tamper, got %v", err)
	}
	board.tamperAfter = false
	mu.Lock()
	if len(completes) != 1 || completes[0].Outcome != "failed" || completes[0].ErrorCode != "conflict" {
		t.Fatalf("expected failed conflict report, got %+v", completes)
	}
	mu.Unlock()

	// 3. Uncertain completion (server conflict / lost response) returns ErrUncertain without replay.
	mu.Lock()
	failComplete = true
	mu.Unlock()
	writesBefore := board.writes
	if _, err := pc.Apply(context.Background(), client, "h1", false); !errors.Is(err, clipboardintegration.ErrUncertain) {
		t.Fatalf("expected ErrUncertain, got %v", err)
	}
	if board.writes != writesBefore+1 {
		t.Fatalf("uncertain completion replayed write: before=%d after=%d", writesBefore, board.writes)
	}
	mu.Lock()
	failComplete = false
	mu.Unlock()

	// 4. File apply keeps lease when OnRelease fires with same paths (clipboard manager takeover),
	//    then releases lease when external clipboard replaces content.
	fileSnap := clip.Snapshot{Files: []clip.File{{Name: "doc.txt", Path: sampleFile, Size: 5}}}
	mu.Lock()
	nextGrant = api.PanelApplyGrant{
		ServiceID: "svc-1",
		Epoch:     "ep-1",
		Token:     engine.ApplyToken("tok-file"),
		Snapshot:  fileSnap,
		HasFiles:  true,
	}
	mu.Unlock()
	mark, err := pc.Apply(context.Background(), client, "h-file", false)
	if err != nil || mark == "" {
		t.Fatalf("file apply failed: mark=%q err=%v", mark, err)
	}
	if !pc.Matches(mark) {
		t.Fatal("expected Matches(mark) to return true right after write")
	}

	// Simulate clipboard manager taking ownership while keeping the exact same file paths.
	board.triggerRelease()
	time.Sleep(80 * time.Millisecond)
	mu.Lock()
	relCount := len(releases)
	mu.Unlock()
	if relCount != 0 {
		t.Fatalf("lease released prematurely while same file paths stayed on clipboard: %+v", releases)
	}

	// External replacement changes clipboard -> Matches becomes false and release fires once.
	board.setExternal(clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("other")}}})
	if pc.Matches(mark) {
		t.Fatal("expected Matches(mark) to be false after external replacement")
	}
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		mu.Lock()
		n := len(releases)
		mu.Unlock()
		if n == 1 {
			break
		}
		time.Sleep(15 * time.Millisecond)
	}
	mu.Lock()
	if len(releases) != 1 || releases[0].Token != engine.ApplyToken("tok-file") {
		t.Fatalf("expected 1 release for tok-file, got %+v", releases)
	}
	mu.Unlock()
}

func TestPanelClipboardReadFailureKeepsHoldAndReleaseRetriesOnClose(t *testing.T) {
	dir, err := os.MkdirTemp("/tmp", "uc-pc-retry-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	sock := filepath.Join(dir, "p.sock")
	ln, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	sampleFile := filepath.Join(dir, "doc.txt")
	if err := os.WriteFile(sampleFile, []byte("hello"), 0600); err != nil {
		t.Fatal(err)
	}
	fileSnap := clip.Snapshot{Files: []clip.File{{Name: "doc.txt", Path: sampleFile, Size: 5}}}

	var mu sync.Mutex
	failRelease := true
	releaseAttempts := 0
	releaseSucceeded := 0

	mux := http.NewServeMux()
	mux.HandleFunc("POST /v1/desktop/panel-apply/begin", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(api.PanelApplyGrant{
			ServiceID: "svc-1",
			Epoch:     "ep-1",
			Token:     engine.ApplyToken("tok-retry"),
			Snapshot:  fileSnap,
			HasFiles:  true,
		})
	})
	mux.HandleFunc("POST /v1/desktop/panel-apply/complete", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(api.PanelApplyCompleteReply{Applied: true})
	})
	mux.HandleFunc("POST /v1/desktop/panel-apply/release", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		releaseAttempts++
		fail := failRelease
		if !fail {
			releaseSucceeded++
		}
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		if fail {
			w.WriteHeader(http.StatusInternalServerError)
			_ = json.NewEncoder(w).Encode(api.ErrorBody{Error: "temp failure", Reason: "unavailable"})
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]bool{"ok": true})
	})

	srv := &http.Server{Handler: mux}
	go func() { _ = srv.Serve(ln) }()
	defer func() { _ = srv.Close() }()

	client := api.NewClient(sock)
	board := &mockPanelBoard{}
	pc := clipboardintegration.NewPanelClipboard(dir, board)

	if _, err := pc.Apply(context.Background(), client, "h-file", false); err != nil {
		t.Fatalf("file apply failed: %v", err)
	}
	// Transient read failure during clipboard event must NOT release the hold.
	board.mu.Lock()
	board.readErr = errors.New("clipboard temporarily locked")
	board.mu.Unlock()
	board.triggerRelease()
	time.Sleep(100 * time.Millisecond)
	mu.Lock()
	gotAttempts := releaseAttempts
	mu.Unlock()
	if gotAttempts != 0 {
		t.Fatalf("expected 0 release calls on transient Read() error, got %d", gotAttempts)
	}

	// Once clipboard read succeeds with replacement content, release is attempted (and fails once).
	board.mu.Lock()
	board.readErr = nil
	board.mu.Unlock()
	board.setExternal(clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("replaced")}}})
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		mu.Lock()
		a := releaseAttempts
		mu.Unlock()
		if a >= 1 {
			break
		}
		time.Sleep(15 * time.Millisecond)
	}
	mu.Lock()
	if releaseAttempts < 1 {
		mu.Unlock()
		t.Fatal("expected release attempt after clipboard replacement")
	}
	failRelease = false
	mu.Unlock()

	// Close() must retry the queued failed release without repeating any clipboard write.
	writesBeforeClose := board.writes
	pc.Close()
	mu.Lock()
	succ := releaseSucceeded
	mu.Unlock()
	if succ != 1 {
		t.Fatalf("expected Close() to retry pending release once, succeeded=%d", succ)
	}
	if board.writes != writesBeforeClose {
		t.Fatalf("Close() replayed clipboard write: before=%d after=%d", writesBeforeClose, board.writes)
	}
}
