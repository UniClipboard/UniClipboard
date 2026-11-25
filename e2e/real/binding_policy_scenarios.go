package main

import (
	"context"
	"encoding/json"
	"fmt"
	"strconv"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type bindingPolicyCheck struct {
	Case         string  `json:"case"`
	Sender       string  `json:"sender"`
	Receiver     string  `json:"receiver"`
	Mode         string  `json:"mode"` // auto | explicit | cli | pause
	Kind         string  `json:"kind"` // text | png | file | control
	Policy       string  `json:"policy"`
	Expected     string  `json:"expected"`
	Observed     string  `json:"observed"`
	Result       string  `json:"result"`
	Milliseconds float64 `json:"milliseconds"`
}

func (c *controller) recordBindingPolicyCheck(row bindingPolicyCheck) error {
	c.manifest.BindingPolicyChecks = append(c.manifest.BindingPolicyChecks, row)
	fmt.Printf("POLICY %-34s %s->%s [%s/%s] %s | exp=%s got=%s (%.1fms)\n",
		row.Case, row.Sender, row.Receiver, row.Mode, row.Kind, row.Result, row.Expected, row.Observed, row.Milliseconds)
	return c.save()
}

func (c *controller) setNodeBindingPolicy(ctx context.Context, src, peer *node, pol engine.BindingSyncPolicy) (engine.DeviceBinding, error) {
	st, err := state(ctx, src)
	if err != nil {
		return engine.DeviceBinding{}, err
	}
	b, ok := bindingForPeer(st, peer.deviceID)
	if !ok {
		return engine.DeviceBinding{}, fmt.Errorf("%s: authorized_binding_missing", src.name)
	}
	res, err := src.cli(ctx, "device", "path", "policy", b.ID, strconv.FormatInt(b.Revision, 10),
		"--text", string(pol.Text),
		"--images", string(pol.Images),
		"--files", string(pol.Files),
	)
	if err != nil {
		return engine.DeviceBinding{}, err
	}
	var updated engine.DeviceBinding
	if err := json.Unmarshal(res.Out, &updated); err != nil {
		return engine.DeviceBinding{}, fmt.Errorf("%s: invalid_binding_policy_json", src.name)
	}
	if updated.Policy != pol || updated.Revision != b.Revision+1 {
		return engine.DeviceBinding{}, fmt.Errorf("%s: binding_policy_response_mismatch", src.name)
	}
	return updated, nil
}

func (c *controller) verifyAutoCell(ctx context.Context, caseName, policyLabel string, src, dst *node, p payload, expectDeliver bool, expectSenderFiltered bool) error {
	seed := digest([]byte(c.manifest.RunID + "/binding-policy/" + caseName + "/" + src.name + "/" + dst.name))[:24]
	f, err := c.prepare(ctx, src, p, seed)
	if err != nil {
		return err
	}
	defer func() { _ = src.action(ctx, "release", f.ID) }()

	if err := dst.action(ctx, "arm", armRequest{Source: src.deviceID, Fixture: f}); err != nil {
		return err
	}
	t0 := time.Now()
	var obs observeAutoResult
	if err := src.call(ctx, "observe_auto", observeAutoRequest{Fixture: f.ID}, &obs); err != nil {
		return fmt.Errorf("%s: observe_auto_failed: %w", caseName, err)
	}
	if expectDeliver {
		if obs.Disposition != engine.DispSubmitted {
			return fmt.Errorf("%s: auto_disposition_%s_want_submitted", caseName, obs.Disposition)
		}
		var del delivery
		if err := dst.call(ctx, "wait", waitRequest{TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &del); err != nil {
			return fmt.Errorf("%s: expected_auto_delivery_failed: %w", caseName, err)
		}
		if del.Count != 1 {
			return fmt.Errorf("%s: unexpected_delivery_count_%d", caseName, del.Count)
		}
		return c.recordBindingPolicyCheck(bindingPolicyCheck{
			Case: caseName, Sender: src.name, Receiver: dst.name, Mode: "auto", Kind: p.Kind,
			Policy: policyLabel, Expected: "delivered+sha256",
			Observed:     fmt.Sprintf("submitted(%s)+verified(%d parts)", obs.Outcome, len(del.Actual)),
			Result:       "passed",
			Milliseconds: float64(time.Since(t0)) / float64(time.Millisecond),
		})
	}

	if expectSenderFiltered && obs.Disposition != engine.DispDisabled {
		return fmt.Errorf("%s: auto_sender_not_filtered_got_%s", caseName, obs.Disposition)
	}
	if err := sleep(ctx, 1500*time.Millisecond); err != nil {
		return err
	}
	var count int
	if err := dst.call(ctx, "audit", nil, &count); err != nil {
		return fmt.Errorf("%s: receiver_audit_failed: %w", caseName, err)
	}
	if count != 0 {
		return fmt.Errorf("%s: blocked_auto_content_leaked_count_%d", caseName, count)
	}
	exp := "blocked_receiver_discard"
	if expectSenderFiltered {
		exp = "blocked_sender_filter"
	}
	return c.recordBindingPolicyCheck(bindingPolicyCheck{
		Case: caseName, Sender: src.name, Receiver: dst.name, Mode: "auto", Kind: p.Kind,
		Policy: policyLabel, Expected: exp,
		Observed:     fmt.Sprintf("disp=%s,recv=0", obs.Disposition),
		Result:       "passed",
		Milliseconds: float64(time.Since(t0)) / float64(time.Millisecond),
	})
}

func (c *controller) verifyExplicitCell(ctx context.Context, caseName, policyLabel string, src, dst *node, p payload, expectDeliver bool) error {
	key := digest([]byte(c.manifest.RunID + "/binding-policy-explicit/" + caseName + "/" + src.name + "/" + dst.name))[:32]
	f, err := c.prepare(ctx, src, p, key)
	if err != nil {
		return err
	}
	defer func() { _ = src.action(ctx, "release", f.ID) }()

	if err := dst.action(ctx, "arm", armRequest{Source: src.deviceID, Fixture: f}); err != nil {
		return err
	}
	t0 := time.Now()
	var sub submitResult
	submitErr := src.call(ctx, "submit", submitRequest{Fixture: f.ID, Key: key}, &sub)
	if expectDeliver {
		if submitErr != nil {
			return fmt.Errorf("%s: explicit_submit_failed: %w", caseName, submitErr)
		}
		var del delivery
		if err := dst.call(ctx, "wait", waitRequest{TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &del); err != nil {
			return fmt.Errorf("%s: explicit_delivery_failed: %w", caseName, err)
		}
		return c.recordBindingPolicyCheck(bindingPolicyCheck{
			Case: caseName, Sender: src.name, Receiver: dst.name, Mode: "explicit", Kind: p.Kind,
			Policy: policyLabel, Expected: "bypass_policy+delivered+sha256",
			Observed:     fmt.Sprintf("cli_exit=%d,verified(%d parts)", sub.InitialCLIExit, len(del.Actual)),
			Result:       "passed",
			Milliseconds: float64(time.Since(t0)) / float64(time.Millisecond),
		})
	}
	if err := sleep(ctx, 1500*time.Millisecond); err != nil {
		return err
	}
	var count int
	if err := dst.call(ctx, "audit", nil, &count); err != nil {
		return fmt.Errorf("%s: explicit_negative_audit_failed: %w", caseName, err)
	}
	if count != 0 {
		return fmt.Errorf("%s: blocked_explicit_content_leaked_count_%d", caseName, count)
	}
	obsText := fmt.Sprintf("cli_exit=%d,recv=0", sub.InitialCLIExit)
	if submitErr != nil {
		obsText = "submit_rejected,recv=0"
	}
	return c.recordBindingPolicyCheck(bindingPolicyCheck{
		Case: caseName, Sender: src.name, Receiver: dst.name, Mode: "explicit", Kind: p.Kind,
		Policy: policyLabel, Expected: "blocked_0_received",
		Observed:     obsText,
		Result:       "passed",
		Milliseconds: float64(time.Since(t0)) / float64(time.Millisecond),
	})
}

func (c *controller) bindingPolicyMatrix(ctx context.Context) error {
	if len(c.nodes) < 2 {
		return fmt.Errorf("binding_policy_requires_two_nodes")
	}
	host, peer := c.pickWakePair()
	if err := c.activate(ctx, []*node{host, peer}); err != nil {
		return err
	}
	if err := ensurePair(ctx, host, peer, nil); err != nil {
		return err
	}

	pText := payload{Kind: "text", Size: 96}
	pPNG := payload{Kind: "png", Size: 32}
	pFile := payload{Kind: "file", Size: 4096}

	// ---- Matrix Block 1: Asymmetric per-type directions on host->peer binding ----
	// host policy: text=receive_only, images=send_only, files=disabled
	// peer policy: default (all bidirectional)
	t0 := time.Now()
	stBefore, err := state(ctx, host)
	if err != nil {
		return err
	}
	bBefore, ok := bindingForPeer(stBefore, peer.deviceID)
	if !ok {
		return fmt.Errorf("binding_missing_before_policy_matrix")
	}
	polAsym := engine.BindingSyncPolicy{
		Text:   engine.SyncDirectionReceiveOnly,
		Images: engine.SyncDirectionSendOnly,
		Files:  engine.SyncDirectionDisabled,
	}
	bAfter, err := c.setNodeBindingPolicy(ctx, host, peer, polAsym)
	if err != nil {
		return err
	}
	// Decision #3: updating policy keeps live session connected (no reconnect drop).
	if bAfter.Connection != engine.Connected {
		return fmt.Errorf("binding_policy_update_dropped_live_session")
	}
	// Stale revision must be rejected by CAS; invalid direction must fail cleanly.
	var staleRes cliResult
	if err := host.call(ctx, "cli", cliRequest{
		Args:         []string{"device", "path", "policy", bBefore.ID, strconv.FormatInt(bBefore.Revision, 10), "--text", "bidirectional"},
		AllowFailure: true,
	}, &staleRes); err != nil || staleRes.Code == 0 {
		return fmt.Errorf("stale_binding_policy_revision_accepted")
	}
	var badDirRes cliResult
	if err := host.call(ctx, "cli", cliRequest{
		Args:         []string{"device", "path", "policy", bAfter.ID, strconv.FormatInt(bAfter.Revision, 10), "--text", "bogus"},
		AllowFailure: true,
	}, &badDirRes); err != nil || badDirRes.Code == 0 {
		return fmt.Errorf("invalid_binding_policy_direction_accepted")
	}
	if err := c.recordBindingPolicyCheck(bindingPolicyCheck{
		Case: "cli_cas_and_live_session_kept", Sender: host.name, Receiver: peer.name, Mode: "cli", Kind: "control",
		Policy: "text=recv,img=send,file=off", Expected: "connected+stale_409+bad_rejected",
		Observed: fmt.Sprintf("conn=%s,rev=%d->%d", bAfter.Connection, bBefore.Revision, bAfter.Revision),
		Result:   "passed", Milliseconds: float64(time.Since(t0)) / float64(time.Millisecond),
	}); err != nil {
		return err
	}

	// Row 1: text = receive_only on host
	if err := c.verifyAutoCell(ctx, "asym_text_receive_only_auto_out", "host:text=recv", host, peer, pText, false, true); err != nil {
		return err
	}
	if err := c.verifyExplicitCell(ctx, "asym_text_receive_only_explicit_out", "host:text=recv", host, peer, pText, true); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "asym_text_receive_only_auto_in", "host:text=recv", peer, host, pText, true, false); err != nil {
		return err
	}

	// Row 2: images = send_only on host
	if err := c.verifyAutoCell(ctx, "asym_png_send_only_auto_out", "host:img=send", host, peer, pPNG, true, false); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "asym_png_send_only_auto_in", "host:img=send", peer, host, pPNG, false, false); err != nil {
		return err
	}
	if err := c.verifyExplicitCell(ctx, "asym_png_send_only_explicit_in", "host:img=send", peer, host, pPNG, false); err != nil {
		return err
	}

	// Row 3: files = disabled on host
	if err := c.verifyAutoCell(ctx, "asym_file_disabled_auto_out", "host:file=off", host, peer, pFile, false, true); err != nil {
		return err
	}
	if err := c.verifyExplicitCell(ctx, "asym_file_disabled_explicit_out", "host:file=off", host, peer, pFile, true); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "asym_file_disabled_auto_in", "host:file=off", peer, host, pFile, false, false); err != nil {
		return err
	}

	// ---- Matrix Block 2: One-way pipe preset (host=all send_only, peer=all receive_only) ----
	allSend := engine.BindingSyncPolicy{Text: engine.SyncDirectionSendOnly, Images: engine.SyncDirectionSendOnly, Files: engine.SyncDirectionSendOnly}
	allRecv := engine.BindingSyncPolicy{Text: engine.SyncDirectionReceiveOnly, Images: engine.SyncDirectionReceiveOnly, Files: engine.SyncDirectionReceiveOnly}
	if _, err := c.setNodeBindingPolicy(ctx, host, peer, allSend); err != nil {
		return err
	}
	if _, err := c.setNodeBindingPolicy(ctx, peer, host, allRecv); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "oneway_pipe_text_forward", "host=send,peer=recv", host, peer, pText, true, false); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "oneway_pipe_png_forward", "host=send,peer=recv", host, peer, pPNG, true, false); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "oneway_pipe_file_forward", "host=send,peer=recv", host, peer, pFile, true, false); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "oneway_pipe_reverse_auto_blocked", "host=send,peer=recv", peer, host, pText, false, true); err != nil {
		return err
	}
	if err := c.verifyExplicitCell(ctx, "oneway_pipe_reverse_explicit_discarded", "host=send,peer=recv", peer, host, pText, false); err != nil {
		return err
	}

	// ---- Matrix Block 3: All-disabled preset ("不同步") still allows explicit manual send ----
	allOff := engine.BindingSyncPolicy{Text: engine.SyncDirectionDisabled, Images: engine.SyncDirectionDisabled, Files: engine.SyncDirectionDisabled}
	allBi := engine.DefaultBindingSyncPolicy()
	if _, err := c.setNodeBindingPolicy(ctx, host, peer, allOff); err != nil {
		return err
	}
	if _, err := c.setNodeBindingPolicy(ctx, peer, host, allBi); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "all_disabled_auto_out_blocked", "host=off,peer=bi", host, peer, pText, false, true); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "all_disabled_auto_in_blocked", "host=off,peer=bi", peer, host, pText, false, false); err != nil {
		return err
	}
	if err := c.verifyExplicitCell(ctx, "all_disabled_explicit_out_bypass", "host=off,peer=bi", host, peer, pText, true); err != nil {
		return err
	}

	// ---- Matrix Block 4: Upper-layer channel policy ceiling (0007 invariant) ----
	if _, err := c.setNodeBindingPolicy(ctx, host, peer, allBi); err != nil {
		return err
	}
	if err := host.action(ctx, "channel_policy", channelPolicyRequest{Text: true, Images: false, Files: true}); err != nil {
		return err
	}
	defer func() {
		_ = host.action(context.Background(), "channel_policy", channelPolicyRequest{Text: true, Images: true, Files: true})
	}()
	if err := c.verifyAutoCell(ctx, "channel_ceiling_png_blocked", "channel:img=false,binding=bi", host, peer, pPNG, false, true); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "channel_ceiling_text_allowed", "channel:img=false,binding=bi", host, peer, pText, true, false); err != nil {
		return err
	}
	if err := host.action(ctx, "channel_policy", channelPolicyRequest{Text: true, Images: true, Files: true}); err != nil {
		return err
	}

	// ---- Matrix Block 5: Master path pause blocks BOTH auto and explicit, then resume restores ----
	stPause, err := state(ctx, host)
	if err != nil {
		return err
	}
	bPause, ok := bindingForPeer(stPause, peer.deviceID)
	if !ok {
		return fmt.Errorf("binding_missing_for_pause")
	}
	pauseOut, err := host.cli(ctx, "device", "path", "pause", bPause.ID, strconv.FormatInt(bPause.Revision, 10))
	if err != nil {
		return err
	}
	var bPaused engine.DeviceBinding
	if err := json.Unmarshal(pauseOut.Out, &bPaused); err != nil || bPaused.SyncEnabled {
		return fmt.Errorf("path_pause_failed")
	}
	if err := c.verifyAutoCell(ctx, "paused_path_auto_blocked", "sync_enabled=false", host, peer, pText, false, false); err != nil {
		return err
	}
	if err := c.verifyExplicitCell(ctx, "paused_path_explicit_blocked", "sync_enabled=false", host, peer, pText, false); err != nil {
		return err
	}
	if _, err := host.cli(ctx, "device", "path", "resume", bPaused.ID, strconv.FormatInt(bPaused.Revision, 10)); err != nil {
		return err
	}
	if err := waitEdge(ctx, host, peer, true); err != nil {
		return err
	}
	if err := c.verifyAutoCell(ctx, "resumed_path_auto_restored", "sync_enabled=true,bi", host, peer, pText, true, false); err != nil {
		return err
	}

	// Verify no secrets leaked in status JSON after all policy mutations.
	stFinal, err := host.cli(ctx, "device", "list", "--json")
	if err != nil {
		return err
	}
	if hasSensitiveNetworkLeak(stFinal.Out) {
		return fmt.Errorf("status_json_leaked_sensitive_data_after_policy_matrix")
	}
	var parsed api.Status
	if err := json.Unmarshal(stFinal.Out, &parsed); err != nil {
		return fmt.Errorf("invalid_final_status_json")
	}
	if bFin, ok := bindingForPeer(parsed, peer.deviceID); !ok || bFin.Policy != allBi {
		return fmt.Errorf("final_binding_policy_mismatch")
	}

	return c.check("binding-policy: real cross-OS matrix verified per-type directions, explicit bypass, upper-layer ceiling, CAS/session preservation and path pause/resume")
}
