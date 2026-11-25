package tcnet

import (
	"testing"
	"time"
)

// TestStartAndCloseConcurrentlyNoRace verifies that closing a Transport while
// Start is still running (e.g. during short-lived daemon shutdown) aborts
// startup promptly without racing tailcat.Server.Start against Close.
func TestStartAndCloseConcurrentlyNoRace(t *testing.T) {
	tr, err := New(t.TempDir(), func(string, ...any) {})
	if err != nil {
		t.Fatal(err)
	}
	done := make(chan struct{})
	go func() {
		defer close(done)
		_ = tr.Start()
	}()
	time.Sleep(15 * time.Millisecond)
	if err := tr.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("Start did not exit promptly after Close")
	}
}
