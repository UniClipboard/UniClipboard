package engine

import "testing"

// Without a peer backend the engine is local-only: clips are committed, the
// sync result says there is no backend, and the network calls are unsupported.
func TestLocalOnlyWithoutPeerBackend(t *testing.T) {
	e, err := New(bg, Options{DataDir: t.TempDir()})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.Close(bg) }()
	if err := e.Start(bg); err != nil {
		t.Fatal(err)
	}
	h := importText(t, e, "local")
	res, err := e.CommitContent(bg, CommitRequest{RequestID: "lo", LeaseID: h.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	if res.SyncDisposition != SyncNoBackend {
		t.Fatalf("sync = %q, want no backend", res.SyncDisposition)
	}
	if _, err := e.CreatePairingCode(bg); code(err) != CodeUnsupported {
		t.Fatalf("pairing code: %v", err)
	}
	if err := e.NotifyNetworkChanged(bg); code(err) != CodeUnsupported {
		t.Fatalf("network changed: %v", err)
	}
}
