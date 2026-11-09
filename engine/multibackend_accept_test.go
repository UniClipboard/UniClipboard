package engine

import (
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
)

// Acceptance checks that cross the single-feature tests: two backends at
// once, with the same peer, concurrent arrivals and one shared storage quota.

// The same device behind two backends is two targets. One backend failing the
// send does not touch the other's target, and the aggregate is partial.
func TestSamePeerOnTwoBackendsKeepsTargetsApart(t *testing.T) {
	a := &fakeBackend{id: "test.a", pol: allTypes, targets: []string{"same"}}
	b := &fakeBackend{id: "test.b", pol: allTypes, targets: []string{"same"}}
	n := multiNode(t, a, b)
	h := importText(t, n.e, "both")
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "two", LeaseID: h.LeaseID})
	if err != nil || r.OperationID == "" {
		t.Fatalf("%+v %v", r, err)
	}
	op, _ := n.e.GetOperation(bg, r.OperationID)
	if len(op.Targets) != 2 {
		t.Fatalf("one device behind two backends must be two targets: %+v", op.Targets)
	}
	a.finish(TargetSent)
	op, _ = n.e.GetOperation(bg, r.OperationID)
	if op.Phase == PhaseCompleted {
		t.Fatalf("completed while b's target is open: %+v", op)
	}
	b.finish(TargetFailed)
	op, _ = n.e.GetOperation(bg, r.OperationID)
	if op.Phase != PhaseCompleted || op.Outcome != OutcomePartial {
		t.Fatalf("op = %+v", op)
	}
	got := map[PluginID]TargetState{}
	for _, tg := range op.Targets {
		got[tg.BackendID] = tg.State
	}
	if got["test.a"] != TargetSent || got["test.b"] != TargetFailed {
		t.Fatalf("states leaked across backends: %+v", got)
	}
}

// Many arrivals of one transfer through both backends at the same time:
// exactly one is committed and nothing stays reserved for the others.
func TestConcurrentDuplicateArrivalsAcrossBackends(t *testing.T) {
	n := dedupNode(t, nil)
	const arrivals = 32
	var admitted atomic.Int32
	var wg sync.WaitGroup
	start := make(chan struct{})
	for i := 0; i < arrivals; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			backend := PluginID("test.a")
			if i%2 == 1 {
				backend = "test.b"
			}
			if receiveVia(n.e, clipReq(backend, "t1", "h1", "same bytes"), "same bytes") {
				admitted.Add(1)
			}
		}()
	}
	close(start)
	wg.Wait()
	if admitted.Load() != 1 || receivedCount(t, n.e) != 1 || historyCount(t, n.e) != 1 {
		t.Fatalf("admitted=%d received=%d history=%d", admitted.Load(), receivedCount(t, n.e), historyCount(t, n.e))
	}
	if stored(n.e) != int64(len("same bytes")) {
		t.Fatalf("stored = %d, a duplicate kept a reservation", stored(n.e))
	}
}

// Both backends draw on one storage quota: concurrent distinct transfers can
// never push the total past it, and refused ones leave no reservation behind.
func TestBackendsShareOneStorageQuota(t *testing.T) {
	const quota, size, arrivals = 100, 30, 12
	n := dedupNode(t, func(o *Options) {
		o.Settings = DefaultSettings()
		o.Settings.Limits.MaxTotalBytes = quota
		o.Resources.MaxStoredBytes = quota
	})
	text := strings.Repeat("x", size)
	var admitted atomic.Int32
	var wg sync.WaitGroup
	start := make(chan struct{})
	for i := 0; i < arrivals; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			backend := PluginID("test.a")
			if i%2 == 1 {
				backend = "test.b"
			}
			if adm, ok := n.e.admitReceive(clipReq(backend, fmt.Sprint("t", i), "h", text)); ok {
				admitted.Add(1)
				_ = adm // keep the reservation: the clip is still arriving
			}
		}()
	}
	close(start)
	wg.Wait()
	if admitted.Load() != quota/size {
		t.Fatalf("admitted %d transfers of %d bytes under a %d byte quota, want %d", admitted.Load(), size, quota, quota/size)
	}
	if got := stored(n.e); got != int64(admitted.Load())*size || got > quota {
		t.Fatalf("stored = %d", got)
	}
}
