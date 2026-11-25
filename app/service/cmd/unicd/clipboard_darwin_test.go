//go:build darwin

package main

import (
	"context"
	"net/http"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/uniclipboard/unic/app/service/internal/host"
	"github.com/uniclipboard/unic/app/service/internal/server"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
	"github.com/uniclipboard/unic/internal/testboard"
)

func TestDarwinOnDemandClipboardEnableNoPreexistingBroadcast(t *testing.T) {
	dir, err := os.MkdirTemp("/private/tmp", "uc-dar-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })

	boardDir := filepath.Join(dir, "board")
	board := testboard.Dir(boardDir)
	preExisting := clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("pre-existing-system-clip")}}}
	if err := board.Write(preExisting); err != nil {
		t.Fatal(err)
	}

	origOpen := openDesktopBoard
	openDesktopBoard = func() (clipboardintegration.Board, error) { return board, nil }
	t.Cleanup(func() { openDesktopBoard = origOpen })

	cfgPath := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(cfgPath, []byte("[telemetry]\nenabled=false\n[history]\nauto_cleanup=false\n[sync]\nenabled=false\n"), 0600); err != nil {
		t.Fatal(err)
	}
	settings, err := config.Open(cfgPath, t.Logf)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	lc := servicelifecycle.New(servicelifecycle.Instance{
		ID:        "gen-darwin-test",
		PID:       os.Getpid(),
		Directory: dir,
		Mode:      servicelifecycle.ModeStandalone,
	}, cancel)
	node, err := host.Open(ctx, host.Options{Dir: dir, Settings: settings, ServiceInfo: lc.Info})
	if err != nil {
		t.Fatal(err)
	}
	if err := node.Start(ctx); err != nil {
		_ = node.Close(context.Background())
		t.Fatal(err)
	}
	defer func() {
		closeCtx, done := context.WithTimeout(context.Background(), 5*time.Second)
		defer done()
		_ = node.Close(closeCtx)
	}()

	sock := filepath.Join(dir, "unic.sock")
	ln, err := server.Listen(sock)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	mux := http.NewServeMux()
	lc.Register(mux)
	mux.Handle("/", server.Handler(node, settings))
	waitClip := registerDesktopClipboard(ctx, mux, sock, lc)
	srv := &http.Server{Handler: mux}
	go func() { _ = srv.Serve(ln) }()
	defer func() {
		cancel()
		_ = srv.Close()
		waitClip()
	}()

	client := api.NewClient(sock)
	stored := clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("stored-history-darwin")}}}
	sub, err := client.Submit(ctx, api.SubmitInput{RequestID: "sub-dar", Snapshot: stored})
	if err != nil {
		t.Fatal(err)
	}

	// Before opt-in: ApplyDesktopClipboard reports failure=clipboard.
	beforeRes, err := client.ApplyDesktopClipboard(ctx, string(sub.HistoryID), false)
	if err != nil || beforeRes.Failure != "clipboard" {
		t.Fatalf("expected failure=clipboard before enable, got res=%+v err=%v", beforeRes, err)
	}

	// F08: On-demand EnableDesktopClipboard succeeds and must NOT broadcast pre-existing clipboard.
	if err := client.EnableDesktopClipboard(ctx); err != nil {
		t.Fatalf("EnableDesktopClipboard: %v", err)
	}
	// Idempotent repeat returns immediately.
	if err := client.EnableDesktopClipboard(ctx); err != nil {
		t.Fatalf("repeat EnableDesktopClipboard: %v", err)
	}
	time.Sleep(600 * time.Millisecond)
	page, err := client.ListHistory(ctx, "", "")
	if err != nil {
		t.Fatal(err)
	}
	if len(page.Items) != 1 {
		t.Fatalf("F08: pre-existing clipboard was broadcast into history: got %d items, want 1", len(page.Items))
	}

	// Now ApplyDesktopClipboard applies stored history and MatchDesktopClipboard verifies it.
	res, err := client.ApplyDesktopClipboard(ctx, string(sub.HistoryID), false)
	if err != nil || res.Failure != "" || res.Mark == "" {
		t.Fatalf("ApplyDesktopClipboard failed: res=%+v err=%v", res, err)
	}
	got, err := board.Read()
	if err != nil || got.Hash() != stored.Hash() {
		t.Fatalf("board mismatch after darwin apply: err=%v got=%+v", err, got)
	}
	matched, err := client.MatchDesktopClipboard(ctx, res.Mark)
	if err != nil || !matched {
		t.Fatalf("expected MatchDesktopClipboard=true, got matched=%v err=%v", matched, err)
	}
	// External replacement invalidates match.
	if err := board.Write(preExisting); err != nil {
		t.Fatal(err)
	}
	matched, err = client.MatchDesktopClipboard(ctx, res.Mark)
	if err != nil || matched {
		t.Fatalf("expected MatchDesktopClipboard=false after replacement, got matched=%v err=%v", matched, err)
	}
}
