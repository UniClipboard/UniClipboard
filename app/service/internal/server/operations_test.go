package server

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type fakeOpsBackend struct {
	fake
	api.HeadlessBackend
	lastFilter  api.OperationsFilter
	ops         []engine.Operation
	discardedID string
	discardErr  error
}

func (f *fakeOpsBackend) Operations(_ context.Context, filter api.OperationsFilter) ([]engine.Operation, error) {
	f.lastFilter = filter
	return f.ops, nil
}

func (f *fakeOpsBackend) DiscardReceive(_ context.Context, id string) error {
	f.discardedID = id
	return f.discardErr
}

func TestOperationsEndpointPassesFiltersAndSerializesProgress(t *testing.T) {
	fb := &fakeOpsBackend{
		ops: []engine.Operation{{
			ID:        "op-1",
			RequestID: "req-1",
			Kind:      engine.OperationSend,
			HistoryID: "hist-1",
			Phase:     engine.PhaseSending,
			Targets: []engine.TargetStatus{{
				BackendID:    engine.BackendTailcat,
				TargetKind:   engine.TargetDevice,
				TargetID:     "dev-b",
				PeerID:       "dev-b",
				State:        engine.TargetSending,
				ReceiptStage: engine.ReceiptStageLocalWritten,
				Progress: &engine.TransferProgress{
					BytesSent:  524288,
					TotalBytes: 1048576,
					PartIndex:  0,
					PartCount:  1,
				},
			}},
		}},
	}
	h := Handler(fb, config.Static(config.Default()))
	rec := do(t, h, "GET", "/v1/operations?active=true&history_id=hist-1", "")
	if rec.Code != 200 {
		t.Fatalf("status = %d body = %s", rec.Code, rec.Body)
	}
	if !fb.lastFilter.Active || fb.lastFilter.HistoryID != "hist-1" {
		t.Fatalf("filter = %+v, want active=true history_id=hist-1", fb.lastFilter)
	}
	var got []engine.Operation
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || got[0].HistoryID != "hist-1" || len(got[0].Targets) != 1 {
		t.Fatalf("got = %+v", got)
	}
	tgt := got[0].Targets[0]
	if tgt.ReceiptStage != engine.ReceiptStageLocalWritten || tgt.Progress == nil || tgt.Progress.BytesSent != 524288 || tgt.Progress.TotalBytes != 1048576 {
		t.Fatalf("target = %+v", tgt)
	}
}

func TestDiscardReceiveEndpointRoutesAndErrors(t *testing.T) {
	fb := &fakeOpsBackend{}
	h := Handler(fb, config.Static(config.Default()))

	rec := do(t, h, "POST", "/v1/operations/op-rx-1/discard", "")
	if rec.Code != 200 {
		t.Fatalf("status = %d body = %s", rec.Code, rec.Body)
	}
	if fb.discardedID != "op-rx-1" {
		t.Fatalf("discardedID = %q, want op-rx-1", fb.discardedID)
	}

	fb.discardErr = &api.Error{Status: 409, Reason: "conflict", Message: "receive operation already finished"}
	rec = do(t, h, "POST", "/v1/operations/op-rx-2/discard", "")
	if rec.Code != 409 {
		t.Fatalf("conflict status = %d body = %s", rec.Code, rec.Body)
	}
}
