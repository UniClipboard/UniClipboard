package desktopui

import (
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func TestTransferTargetItemStatesAndReceipts(t *testing.T) {
	d := &viewState{
		language: "zh-CN",
		devices: []device{
			{id: "dev-mac", deviceID: "dev-mac", name: "MacBook Pro"},
			{id: "dev-win", deviceID: "dev-win", name: "Workstation"},
		},
	}

	sending := d.transferTargetItem("op1", engine.TargetStatus{
		BackendID: engine.BackendTailcat,
		TargetID:  "dev-mac",
		State:     engine.TargetSending,
		Progress:  &engine.TransferProgress{BytesSent: 50 * 1024 * 1024, TotalBytes: 100 * 1024 * 1024, PartCount: 1},
	})
	if sending.Label != "MacBook Pro" || sending.Tone != desktopkit.StatusInfo || sending.Status != "50%" || sending.Value == nil || *sending.Value != 50 {
		t.Fatalf("sending item = %+v", sending)
	}
	if !strings.Contains(sending.Bytes, "50.0 MB") || !strings.Contains(sending.Bytes, "100.0 MB") {
		t.Fatalf("sending bytes = %q", sending.Bytes)
	}

	awaiting := d.transferTargetItem("op1", engine.TargetStatus{
		BackendID:    engine.BackendTailcat,
		TargetID:     "dev-mac",
		State:        engine.TargetSending,
		ReceiptStage: engine.ReceiptStageLocalWritten,
		Progress:     &engine.TransferProgress{BytesSent: 100 * 1024 * 1024, TotalBytes: 100 * 1024 * 1024},
	})
	if awaiting.Tone != desktopkit.StatusInfo || awaiting.Status != "等待对端确认…" || awaiting.Value != nil {
		t.Fatalf("awaiting item = %+v", awaiting)
	}

	received := d.transferTargetItem("op1", engine.TargetStatus{
		BackendID:    engine.BackendTailcat,
		TargetID:     "dev-mac",
		State:        engine.TargetSent,
		ReceiptStage: engine.ReceiptStageRemoteReceived,
	})
	if received.Tone != desktopkit.StatusPositive || received.Status != "对端已接收" {
		t.Fatalf("received item = %+v", received)
	}

	discarded := d.transferTargetItem("op1", engine.TargetStatus{
		BackendID:    engine.BackendTailcat,
		TargetID:     "dev-win",
		State:        engine.TargetDiscarded,
		ReceiptStage: engine.ReceiptStageRemoteDiscarded,
	})
	if discarded.Label != "Workstation" || discarded.Tone != desktopkit.StatusWarning || discarded.Status != "对端未接纳" {
		t.Fatalf("discarded item = %+v", discarded)
	}

	unconfirmed := d.transferTargetItem("op1", engine.TargetStatus{
		BackendID:    engine.BackendTailcat,
		TargetID:     "dev-win",
		State:        engine.TargetFailed,
		ReceiptStage: engine.ReceiptStageLocalWritten,
		Error:        &engine.Error{Code: engine.CodeTimeout},
	})
	if unconfirmed.Tone != desktopkit.StatusCritical || unconfirmed.Status != "未收到对端确认" || unconfirmed.Reason == "" {
		t.Fatalf("unconfirmed item = %+v", unconfirmed)
	}
}

func TestLiveDeliverySectionRendersPerDeviceProgressAndReceipt(t *testing.T) {
	nowMs := time.Now().UnixMilli()
	d := &viewState{
		language: "zh-CN",
		devices:  []device{{id: "dev-mac", deviceID: "dev-mac", name: "MacBook Pro"}},
		live:     &liveState{opsByHistory: map[string]engine.Operation{}},
	}
	d.live.applyOperations([]engine.Operation{{
		ID:          "op-file-1",
		Kind:        engine.OperationSend,
		HistoryID:   "hist-file-1",
		Phase:       engine.PhaseSending,
		UpdatedAtMs: nowMs,
		Targets: []engine.TargetStatus{{
			BackendID: engine.BackendTailcat,
			TargetID:  "dev-mac",
			State:     engine.TargetSending,
			Progress:  &engine.TransferProgress{BytesSent: 40 << 20, TotalBytes: 100 << 20, PartCount: 1},
		}},
	}})

	if len(d.live.activeOps) != 1 {
		t.Fatalf("activeOps = %d, want 1", len(d.live.activeOps))
	}

	k := desktopkit.Kit{}
	detailEntry := &entry{
		id:        "hist-file-1",
		syncState: "pending",
		backendSync: []api.HistoryBackendSync{{
			BackendID:   "e2e.desktop",
			State:       "pending",
			Disposition: "queued",
		}},
	}
	tester := ui.NewTester(func(c *ui.Context) {
		k.Apply(c)
		ui.Column(c).Children(func() {
			d.toolbarTransferStatus(c, k)
			d.renderDeliverySection(c.Key("delivery"), k, detailEntry, true)
		})
	}, 600, 400)
	tester.Frame()
	if !tester.HasText("MacBook Pro") || !tester.HasText("40%") {
		t.Fatalf("expected per-device 40%% transfer item in delivery section, texts=%v", tester.Texts())
	}
	if !tester.HasText("e2e.desktop") {
		t.Fatalf("expected per-backend summary to remain visible alongside operation targets, texts=%v", tester.Texts())
	}
	if _, ok := tester.Find("传输进度"); !ok {
		t.Fatalf("expected active transfer status indicator in toolbar, texts=%v", tester.Texts())
	}

	// Transition to remote_received and verify UI updates to "对端已接收".
	d.live.applyOperations([]engine.Operation{{
		ID:          "op-file-1",
		Kind:        engine.OperationSend,
		HistoryID:   "hist-file-1",
		Phase:       engine.PhaseCompleted,
		Outcome:     engine.OutcomeSuccess,
		UpdatedAtMs: nowMs,
		Targets: []engine.TargetStatus{{
			BackendID:    engine.BackendTailcat,
			TargetID:     "dev-mac",
			State:        engine.TargetSent,
			ReceiptStage: engine.ReceiptStageRemoteReceived,
			Progress:     &engine.TransferProgress{BytesSent: 100 << 20, TotalBytes: 100 << 20, PartCount: 1},
		}},
	}})
	tester.Frame()
	if !tester.HasText("对端已接收") {
		t.Fatalf("expected MacBook Pro receipt confirmation label after remote_received, texts=%v", tester.Texts())
	}
}

func TestReceiveProgressToastThresholdProgressDismissAndCompletion(t *testing.T) {
	nowMs := time.Now().UnixMilli()
	d := &viewState{
		language: "zh-CN",
		devices:  []device{{id: "dev-mac", deviceID: "dev-mac", name: "macOS (MacBook)"}},
		live: &liveState{
			opsByHistory:      map[string]engine.Operation{},
			discardedReceives: map[string]struct{}{},
		},
	}

	// Small text receive (< 256 KiB) must NOT open a toast.
	d.live.applyOperations([]engine.Operation{{
		ID:          "op-rx-small",
		Kind:        engine.OperationReceive,
		Phase:       engine.PhaseReceiving,
		UpdatedAtMs: nowMs,
		Receive: &engine.ReceiveOperation{
			BackendID:   engine.BackendTailcat,
			PeerID:      "dev-mac",
			State:       engine.ReceiveReceiving,
			ContentKind: "text",
			Progress:    engine.ReceiveProgress{BytesReceived: 64, TotalBytes: 128, PartCount: 1},
		},
	}})
	if len(d.live.activeReceives) != 0 {
		t.Fatalf("small text should not trigger receive toast: %+v", d.live.activeReceives)
	}

	// File receive triggers persistent progress toast.
	d.live.applyOperations([]engine.Operation{{
		ID:          "op-rx-file",
		Kind:        engine.OperationReceive,
		Phase:       engine.PhaseReceiving,
		UpdatedAtMs: nowMs,
		Receive: &engine.ReceiveOperation{
			BackendID:   engine.BackendTailcat,
			PeerID:      "dev-mac",
			State:       engine.ReceiveReceiving,
			ContentKind: "file",
			FileCount:   1,
			Progress:    engine.ReceiveProgress{BytesReceived: 40 << 20, TotalBytes: 100 << 20, PartCount: 1},
		},
	}})

	k := desktopkit.Kit{}
	tester := ui.NewTester(func(c *ui.Context) {
		k.Apply(c)
		d.toasterOverlay(c, k)
	}, 900, 600)
	tester.Frame()

	if !tester.HasText("正在接收来自 macOS (MacBook) 的文件") || !tester.HasText("40%") {
		t.Fatalf("expected receive progress toast with 40%%, texts=%v", tester.Texts())
	}

	// Natural completion auto-dismisses the toast WITHOUT recording local discard.
	d.live.applyOperations([]engine.Operation{{
		ID:          "op-rx-file",
		Kind:        engine.OperationReceive,
		Phase:       engine.PhaseCompleted,
		UpdatedAtMs: nowMs,
		Receive: &engine.ReceiveOperation{
			BackendID:   engine.BackendTailcat,
			PeerID:      "dev-mac",
			State:       engine.ReceiveCommitted,
			ContentKind: "file",
			FileCount:   1,
			Progress:    engine.ReceiveProgress{BytesReceived: 100 << 20, TotalBytes: 100 << 20, PartCount: 1},
		},
	}})
	tester.Frame()
	if d.toasts.Has(toastIDReceivePrefix + "op-rx-file") {
		t.Fatal("receive toast should auto-dismiss on natural commit")
	}
	if _, discarded := d.live.discardedReceives["op-rx-file"]; discarded {
		t.Fatal("natural completion must NOT mark receive as discarded")
	}

	// User clicking × ("放弃接收") discards the active receive immediately.
	d.live.applyOperations([]engine.Operation{{
		ID:          "op-rx-file-2",
		Kind:        engine.OperationReceive,
		Phase:       engine.PhaseReceiving,
		UpdatedAtMs: nowMs,
		Receive: &engine.ReceiveOperation{
			BackendID:   engine.BackendTailcat,
			PeerID:      "dev-mac",
			State:       engine.ReceiveReceiving,
			ContentKind: "file",
			FileCount:   1,
			Progress:    engine.ReceiveProgress{BytesReceived: 20 << 20, TotalBytes: 100 << 20, PartCount: 1},
		},
	}})
	tester.Frame()
	if !tester.HasText("20%") {
		t.Fatalf("expected second receive toast at 20%%, texts=%v", tester.Texts())
	}
	if err := tester.Click("放弃接收"); err != nil {
		t.Fatalf("click discard button: %v", err)
	}
	tester.Frame()
	if d.toasts.Has(toastIDReceivePrefix + "op-rx-file-2") {
		t.Fatal("receive toast should disappear immediately after clicking discard")
	}
	if _, discarded := d.live.discardedReceives["op-rx-file-2"]; !discarded {
		t.Fatal("clicking discard close button must mark receive as discarded")
	}
}
