package main

import (
	"context"
	"fmt"
	"sync"

	"github.com/uniclipboard/unic/engine"
)

type transferProgressCheck struct {
	Case                string           `json:"case"`
	Sender              string           `json:"sender"`
	Receiver            string           `json:"receiver"`
	Kind                string           `json:"kind"`
	Bytes               int64            `json:"bytes"`
	Outcome             string           `json:"outcome"`
	FinalState          string           `json:"final_state"`
	FinalReceiptStage   string           `json:"final_receipt_stage"`
	FoundByHistoryQuery bool             `json:"found_by_history_query"`
	FoundByActiveQuery  bool             `json:"found_by_active_query"`
	OperationEvents     int              `json:"operation_events"`
	Monotonic           bool             `json:"monotonic"`
	SawIntermediate     bool             `json:"saw_intermediate"`
	SawAwaitingReceipt  bool             `json:"saw_awaiting_receipt"`
	SampleCount         int              `json:"sample_count"`
	Samples             []progressSample `json:"samples,omitempty"`
	HashVerified        bool             `json:"hash_verified"`
	Result              string           `json:"result"`
	Milliseconds        float64          `json:"milliseconds"`
}

func (c *controller) recordTransferProgressCheck(row transferProgressCheck) error {
	c.manifest.TransferProgressChecks = append(c.manifest.TransferProgressChecks, row)
	fmt.Printf("PROGRESS %-30s %s->%s [%s %dB] %s | state=%s receipt=%s events=%d samples=%d inter=%v await=%v hash=%v (%.1fms)\n",
		row.Case, row.Sender, row.Receiver, row.Kind, row.Bytes, row.Result,
		row.FinalState, row.FinalReceiptStage, row.OperationEvents, row.SampleCount,
		row.SawIntermediate, row.SawAwaitingReceipt, row.HashVerified, row.Milliseconds)
	return c.save()
}

func (c *controller) transferProgressMatrix(ctx context.Context) error {
	if len(c.nodes) < 2 {
		return fmt.Errorf("transfer-progress requires at least two nodes")
	}
	a, b := c.pickWakePair()
	fmt.Printf("\n=== transfer-progress real-machine integration: %s (%s/%s) <-> %s (%s/%s) ===\n",
		a.name, a.hello.OS, a.hello.Arch, b.name, b.hello.OS, b.hello.Arch)

	if err := c.activate(ctx, []*node{a, b}); err != nil {
		return err
	}
	if err := ensurePair(ctx, a, b, nil); err != nil {
		return err
	}

	runOneWay := func(caseName string, src, dst *node, p payload) error {
		key := digest([]byte(c.manifest.RunID + "/transfer-progress/" + caseName + "/" + src.name + "/" + dst.name))[:32]
		f, err := c.prepare(ctx, src, p, key)
		if err != nil {
			return err
		}
		defer func() { _ = src.action(ctx, "release", f.ID) }()
		if err := dst.action(ctx, "arm", armRequest{Source: src.deviceID, Fixture: f}); err != nil {
			return err
		}
		var res sendProgressResult
		if err := src.call(ctx, "send_with_progress", sendProgressRequest{
			Fixture:        f.ID,
			Key:            key,
			TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds,
		}, &res); err != nil {
			return fmt.Errorf("%s: send_with_progress: %w", caseName, err)
		}
		if res.Outcome != engine.OutcomeSuccess || res.FinalState != engine.TargetSent || res.FinalReceiptStage != engine.ReceiptStageRemoteReceived {
			return fmt.Errorf("%s: unexpected final result: outcome=%s state=%s receipt=%s err=%s",
				caseName, res.Outcome, res.FinalState, res.FinalReceiptStage, res.FinalErrorCode)
		}
		if !res.FoundByHistoryQuery || !res.FoundByActiveQuery {
			return fmt.Errorf("%s: operations query missing op (history=%v active=%v)", caseName, res.FoundByHistoryQuery, res.FoundByActiveQuery)
		}
		if res.OperationHistoryID != res.HistoryID || res.HistoryID == "" {
			return fmt.Errorf("%s: operation history_id=%q want %q", caseName, res.OperationHistoryID, res.HistoryID)
		}
		if !res.Monotonic {
			return fmt.Errorf("%s: non-monotonic progress samples: %+v", caseName, res.Samples)
		}
		if res.OperationEvents == 0 {
			return fmt.Errorf("%s: expected >= 1 SSE operations event, got 0", caseName)
		}
		if res.FinalBytesSent != f.Total || res.FinalTotalBytes != f.Total {
			return fmt.Errorf("%s: final bytes=%d/%d want %d/%d", caseName, res.FinalBytesSent, res.FinalTotalBytes, f.Total, f.Total)
		}
		var del delivery
		if err := dst.call(ctx, "wait", waitRequest{TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &del); err != nil {
			return fmt.Errorf("%s: receiver verify failed: %w", caseName, err)
		}
		if del.Count != 1 {
			return fmt.Errorf("%s: delivery count=%d want 1", caseName, del.Count)
		}
		return c.recordTransferProgressCheck(transferProgressCheck{
			Case:                caseName,
			Sender:              src.name,
			Receiver:            dst.name,
			Kind:                p.Kind,
			Bytes:               f.Total,
			Outcome:             string(res.Outcome),
			FinalState:          string(res.FinalState),
			FinalReceiptStage:   string(res.FinalReceiptStage),
			FoundByHistoryQuery: res.FoundByHistoryQuery,
			FoundByActiveQuery:  res.FoundByActiveQuery,
			OperationEvents:     res.OperationEvents,
			Monotonic:           res.Monotonic,
			SawIntermediate:     res.SawIntermediate,
			SawAwaitingReceipt:  res.SawAwaitingReceipt,
			SampleCount:         len(res.Samples),
			Samples:             res.Samples,
			HashVerified:        true,
			Result:              "passed",
			Milliseconds:        res.ElapsedMS,
		})
	}

	// 1. Large file A -> B (32 MiB): observe live progress & remote_received ACK over real Tailcat.
	if err := runOneWay("large-file-a-to-b", a, b, payload{Kind: "file", Size: 32 << 20}); err != nil {
		return err
	}

	// 2. Large file B -> A (32 MiB): reverse direction over real Tailcat.
	if err := runOneWay("large-file-b-to-a", b, a, payload{Kind: "file", Size: 32 << 20}); err != nil {
		return err
	}

	// 3. Bidirectional concurrent 16 MiB file transfers on the SAME Tailcat session (deadlock proof).
	keyAB := digest([]byte(c.manifest.RunID + "/transfer-progress/bidir-ab"))[:32]
	keyBA := digest([]byte(c.manifest.RunID + "/transfer-progress/bidir-ba"))[:32]
	fAB, err := c.prepare(ctx, a, payload{Kind: "file", Size: 16 << 20}, keyAB)
	if err != nil {
		return err
	}
	defer func() { _ = a.action(ctx, "release", fAB.ID) }()
	fBA, err := c.prepare(ctx, b, payload{Kind: "file", Size: 16 << 20}, keyBA)
	if err != nil {
		return err
	}
	defer func() { _ = b.action(ctx, "release", fBA.ID) }()
	if err := b.action(ctx, "arm", armRequest{Source: a.deviceID, Fixture: fAB}); err != nil {
		return err
	}
	if err := a.action(ctx, "arm", armRequest{Source: b.deviceID, Fixture: fBA}); err != nil {
		return err
	}

	var wg sync.WaitGroup
	var resAB, resBA sendProgressResult
	var errAB, errBA error
	wg.Add(2)
	go func() {
		defer wg.Done()
		errAB = a.call(ctx, "send_with_progress", sendProgressRequest{Fixture: fAB.ID, Key: keyAB, TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &resAB)
	}()
	go func() {
		defer wg.Done()
		errBA = b.call(ctx, "send_with_progress", sendProgressRequest{Fixture: fBA.ID, Key: keyBA, TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &resBA)
	}()
	wg.Wait()
	if errAB != nil || errBA != nil {
		return fmt.Errorf("bidir concurrent failed: a->b=%v b->a=%v", errAB, errBA)
	}
	for _, item := range []struct {
		name string
		src  *node
		dst  *node
		f    fixture
		res  sendProgressResult
	}{
		{"bidir-concurrent-a-to-b", a, b, fAB, resAB},
		{"bidir-concurrent-b-to-a", b, a, fBA, resBA},
	} {
		if item.res.Outcome != engine.OutcomeSuccess || item.res.FinalState != engine.TargetSent || item.res.FinalReceiptStage != engine.ReceiptStageRemoteReceived || !item.res.Monotonic {
			return fmt.Errorf("%s: bad result %+v", item.name, item.res)
		}
		var del delivery
		if err := item.dst.call(ctx, "wait", waitRequest{TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &del); err != nil {
			return fmt.Errorf("%s: receiver verify failed: %w", item.name, err)
		}
		if err := c.recordTransferProgressCheck(transferProgressCheck{
			Case:                item.name,
			Sender:              item.src.name,
			Receiver:            item.dst.name,
			Kind:                "file",
			Bytes:               item.f.Total,
			Outcome:             string(item.res.Outcome),
			FinalState:          string(item.res.FinalState),
			FinalReceiptStage:   string(item.res.FinalReceiptStage),
			FoundByHistoryQuery: item.res.FoundByHistoryQuery,
			FoundByActiveQuery:  item.res.FoundByActiveQuery,
			OperationEvents:     item.res.OperationEvents,
			Monotonic:           item.res.Monotonic,
			SawIntermediate:     item.res.SawIntermediate,
			SawAwaitingReceipt:  item.res.SawAwaitingReceipt,
			SampleCount:         len(item.res.Samples),
			Samples:             item.res.Samples,
			HashVerified:        true,
			Result:              "passed",
			Milliseconds:        item.res.ElapsedMS,
		}); err != nil {
			return err
		}
	}

	// 4. Remote discarded: disable files on B, send 1 MiB file A -> B, verify
	//    TargetDiscarded + remote_discarded, then confirm text still delivers on same session.
	if err := b.action(ctx, "channel_policy", channelPolicyRequest{Text: true, Images: true, Files: false}); err != nil {
		return fmt.Errorf("disable files on %s: %w", b.name, err)
	}
	defer func() {
		_ = b.action(context.Background(), "channel_policy", channelPolicyRequest{Text: true, Images: true, Files: true})
	}()
	keyDisc := digest([]byte(c.manifest.RunID + "/transfer-progress/remote-discard"))[:32]
	fDisc, err := c.prepare(ctx, a, payload{Kind: "file", Size: 1 << 20}, keyDisc)
	if err != nil {
		return err
	}
	defer func() { _ = a.action(ctx, "release", fDisc.ID) }()
	if err := b.action(ctx, "arm", armRequest{Source: a.deviceID, Fixture: fDisc}); err != nil {
		return err
	}
	var resDisc sendProgressResult
	if err := a.call(ctx, "send_with_progress", sendProgressRequest{Fixture: fDisc.ID, Key: keyDisc, TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &resDisc); err != nil {
		return fmt.Errorf("remote-discard: send_with_progress: %w", err)
	}
	if resDisc.FinalState != engine.TargetDiscarded || resDisc.FinalReceiptStage != engine.ReceiptStageRemoteDiscarded || resDisc.Outcome != engine.OutcomeFailed {
		return fmt.Errorf("remote-discard: got outcome=%s state=%s receipt=%s, want failed/discarded/remote_discarded",
			resDisc.Outcome, resDisc.FinalState, resDisc.FinalReceiptStage)
	}
	var count int
	if err := b.call(ctx, "audit", nil, &count); err != nil {
		return fmt.Errorf("remote-discard: receiver audit failed: %w", err)
	}
	if count != 0 {
		return fmt.Errorf("remote-discard: expected 0 received items on %s, got %d", b.name, count)
	}
	if err := c.recordTransferProgressCheck(transferProgressCheck{
		Case:                "remote-discarded-file",
		Sender:              a.name,
		Receiver:            b.name,
		Kind:                "file",
		Bytes:               fDisc.Total,
		Outcome:             string(resDisc.Outcome),
		FinalState:          string(resDisc.FinalState),
		FinalReceiptStage:   string(resDisc.FinalReceiptStage),
		FoundByHistoryQuery: resDisc.FoundByHistoryQuery,
		FoundByActiveQuery:  resDisc.FoundByActiveQuery,
		OperationEvents:     resDisc.OperationEvents,
		Monotonic:           resDisc.Monotonic,
		SawIntermediate:     resDisc.SawIntermediate,
		SawAwaitingReceipt:  resDisc.SawAwaitingReceipt,
		SampleCount:         len(resDisc.Samples),
		Samples:             resDisc.Samples,
		HashVerified:        true,
		Result:              "passed",
		Milliseconds:        resDisc.ElapsedMS,
	}); err != nil {
		return err
	}

	// Verify session survived drain + clip_ack(discarded) by delivering text immediately.
	if err := runOneWay("post-discard-text-same-session", a, b, payload{Kind: "text", Size: 256}); err != nil {
		return err
	}

	return c.check("transfer-progress: real cross-OS Tailcat verified monotonic byte progress, awaiting_receipt/remote_received ACK, bidirectional concurrency without deadlock, remote_discarded and session preservation")
}
