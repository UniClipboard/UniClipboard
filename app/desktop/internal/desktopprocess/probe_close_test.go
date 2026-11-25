package desktopprocess

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"
)

// TestProbeHandlesOwnerCancelAndCloseRace verifies that probing a role
// immediately after a ReplyAfter quit (including when the owner's context is
// cancelled before Owner.Close runs, as in native OnWillQuit) converges cleanly
// on ErrUnavailable without surfacing transient EOF or ECONNRESET errors.
func TestProbeHandlesOwnerCancelAndCloseRace(t *testing.T) {
	dir := t.TempDir()
	for i := range 20 {
		ownerCtx, ownerCancel := context.WithCancel(context.Background())
		closed := make(chan struct{})
		var owner *Owner
		var err error
		owner, err = Acquire(ownerCtx, dir, QuickPanel, func(_ context.Context, command string, _ json.RawMessage) (any, error) {
			if command == "quit" {
				return ReplyAfter(nil, func() {
					ownerCancel()
					time.Sleep(5 * time.Millisecond)
					_ = owner.Close()
					close(closed)
				}), nil
			}
			return nil, nil
		})
		if err != nil {
			ownerCancel()
			t.Fatalf("iteration %d: Acquire: %v", i, err)
		}

		callCtx, cancelCall := context.WithTimeout(context.Background(), 5*time.Second)
		if err := Call(callCtx, dir, QuickPanel, "quit", nil, nil); err != nil {
			cancelCall()
			ownerCancel()
			_ = owner.Close()
			t.Fatalf("iteration %d: Call quit: %v", i, err)
		}
		for {
			_, probeErr := Probe(callCtx, dir, QuickPanel)
			if errors.Is(probeErr, ErrUnavailable) {
				break
			}
			if probeErr != nil {
				cancelCall()
				ownerCancel()
				_ = owner.Close()
				t.Fatalf("iteration %d: unexpected Probe error during shutdown: %v", i, probeErr)
			}
			time.Sleep(2 * time.Millisecond)
		}
		cancelCall()
		<-closed
	}
}
