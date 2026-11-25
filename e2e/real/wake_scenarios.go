package main

import (
	"context"
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type wakeCheck struct {
	Node         string  `json:"node"`
	Peer         string  `json:"peer"`
	Case         string  `json:"case"`
	Result       string  `json:"result"`
	Milliseconds float64 `json:"milliseconds"`
	Path         string  `json:"path,omitempty"`
	LatencyMS    int64   `json:"latency_ms,omitempty"`
	Detail       string  `json:"detail,omitempty"`
}

var rawEndpointPattern = regexp.MustCompile(`\b\d{1,3}(?:\.\d{1,3}){3}:\d+\b`)

func hasSensitiveNetworkLeak(raw []byte) bool {
	s := string(raw)
	return strings.Contains(s, "unic-pair:") || rawEndpointPattern.MatchString(s)
}

func bindingForPeer(st api.Status, peerID string) (engine.DeviceBinding, bool) {
	for _, p := range st.Peers {
		if p.ID != peerID {
			continue
		}
		for _, b := range p.Bindings {
			if b.State == engine.BindingAuthorized {
				return b, true
			}
		}
	}
	return engine.DeviceBinding{}, false
}

func validObservedPath(path string) bool {
	switch path {
	case "direct", "derp", "peer_relay":
		return true
	}
	return false
}

func (c *controller) recordWakeCheck(w wakeCheck) error {
	c.manifest.WakeChecks = append(c.manifest.WakeChecks, w)
	fmt.Printf("WAKE %s %s %s %.1fms %s\n", w.Case, w.Node, w.Result, w.Milliseconds, w.Detail)
	return c.save()
}

func (c *controller) pickWakePair() (host, peer *node) {
	for _, n := range c.nodes {
		if n.config.Transport == "parallels" {
			peer = n
			break
		}
	}
	if peer != nil {
		for _, n := range c.nodes {
			if n != peer {
				host = n
				break
			}
		}
		return host, peer
	}
	return c.nodes[0], c.nodes[1]
}

func (c *controller) wakeTransfer(ctx context.Context, caseName string, src, dst *node) (submitResult, float64, error) {
	key := digest([]byte(c.manifest.RunID + "/wake-refresh/" + caseName + "/" + src.name + "/" + dst.name))[:32]
	f, err := c.prepare(ctx, src, payload{Kind: "text", Size: 64}, key)
	if err != nil {
		return submitResult{}, 0, err
	}
	defer func() { _ = src.action(ctx, "release", f.ID) }()
	start := time.Now()
	out, err := c.transfer(ctx, "wake-refresh", src, []*node{dst}, f, key, nil, 1, 64)
	return out, float64(time.Since(start)) / float64(time.Millisecond), err
}

func (c *controller) wakeRefresh(ctx context.Context) error {
	if len(c.nodes) < 2 {
		return fmt.Errorf("wake_refresh_requires_two_nodes")
	}
	host, peer := c.pickWakePair()
	if err := c.activate(ctx, []*node{host, peer}); err != nil {
		return err
	}
	if err := ensurePair(ctx, host, peer, nil); err != nil {
		return err
	}

	// 1. Manual refresh on both sides via production CLI (`unic device list --refresh`).
	for _, pair := range [][2]*node{{host, peer}, {peer, host}} {
		src, dst := pair[0], pair[1]
		t0 := time.Now()
		res, err := src.cli(ctx, "device", "list", "--refresh", "--json")
		elapsed := float64(time.Since(t0)) / float64(time.Millisecond)
		if err != nil {
			return err
		}
		if hasSensitiveNetworkLeak(res.Out) {
			return fmt.Errorf("%s: status_json_leaked_endpoint_or_ticket", src.name)
		}
		var st api.Status
		if err := json.Unmarshal(res.Out, &st); err != nil {
			return fmt.Errorf("%s: invalid_refresh_status_json", src.name)
		}
		b, ok := bindingForPeer(st, dst.deviceID)
		if !ok || b.Connection != engine.Connected {
			return fmt.Errorf("%s: refresh_peer_not_connected", src.name)
		}
		if !validObservedPath(b.ConnectionPath) {
			return fmt.Errorf("%s: refresh_missing_finite_path", src.name)
		}
		if b.LatencyMs <= 0 || b.LastCheckedMs <= 0 {
			return fmt.Errorf("%s: refresh_missing_rtt_or_timestamp", src.name)
		}
		if err := c.recordWakeCheck(wakeCheck{
			Node: src.name, Peer: dst.name, Case: "manual_refresh_cli",
			Result: "passed", Milliseconds: elapsed, Path: b.ConnectionPath, LatencyMS: b.LatencyMs,
			Detail: fmt.Sprintf("path=%s rtt=%dms privacy=clean", b.ConnectionPath, b.LatencyMs),
		}); err != nil {
			return err
		}
	}
	if _, err := host.cli(ctx, "device", "list", "--refresh"); err != nil {
		return err
	}
	if err := c.check("wake-refresh: manual refresh reports finite path, RTT, timestamp and no raw endpoints on both peers"); err != nil {
		return err
	}

	// 2. Verified baseline transfer before wake/freeze scenarios.
	baseOut, baseMS, err := c.wakeTransfer(ctx, "baseline", host, peer)
	if err != nil {
		return err
	}
	if err := c.recordWakeCheck(wakeCheck{
		Node: host.name, Peer: peer.name, Case: "baseline_transfer",
		Result: "passed", Milliseconds: baseMS,
		Detail: fmt.Sprintf("send_op=%.1fms verified=%.1fms", baseOut.SendOperationMS, baseMS),
	}); err != nil {
		return err
	}

	// 3. Parallels VM freeze/wake fault injection when a Parallels node is present.
	if peer.config.Transport != "parallels" {
		// On non-Parallels pairs, verify NotifyNetworkChanged live warmup + immediate transfer.
		stBefore, err := state(ctx, host)
		if err != nil {
			return err
		}
		bBefore, _ := bindingForPeer(stBefore, peer.deviceID)
		t0 := time.Now()
		if err := host.action(ctx, "notify_network", nil); err != nil {
			return err
		}
		var bWarm engine.DeviceBinding
		if err := poll(ctx, "live_wake_warmup", func() (bool, error) {
			st, err := state(ctx, host)
			if err != nil {
				return false, err
			}
			b, ok := bindingForPeer(st, peer.deviceID)
			if ok && b.Connection == engine.Connected && b.LastCheckedMs > bBefore.LastCheckedMs {
				bWarm = b
				return true, nil
			}
			return false, nil
		}); err != nil {
			return err
		}
		warmMS := float64(time.Since(t0)) / float64(time.Millisecond)
		if err := c.recordWakeCheck(wakeCheck{
			Node: host.name, Peer: peer.name, Case: "live_warmup_probe",
			Result: "passed", Milliseconds: warmMS, Path: bWarm.ConnectionPath, LatencyMS: bWarm.LatencyMs,
			Detail: "non-parallels pair: verified live warmup without VM freeze",
		}); err != nil {
			return err
		}
		out, ms, err := c.wakeTransfer(ctx, "post-live-warmup", host, peer)
		if err != nil {
			return err
		}
		if err := c.recordWakeCheck(wakeCheck{
			Node: host.name, Peer: peer.name, Case: "post_warmup_transfer",
			Result: "passed", Milliseconds: ms,
			Detail: fmt.Sprintf("send_op=%.1fms verified=%.1fms", out.SendOperationMS, ms),
		}); err != nil {
			return err
		}
		return c.check("wake-refresh: live network-change warmup and post-warmup verified transfer succeeded")
	}

	// Ensure peer VM is always resumed on any error path.
	resumePeer := func() error {
		if !peer.paused.Swap(false) {
			return nil
		}
		rctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
		defer cancel()
		return parallelsResume(rctx, peer.config.VM)
	}
	defer func() { _ = resumePeer() }()

	// Scenario A: Freeze VM (half-open blackhole) + host NotifyNetworkChanged -> fast zombie teardown.
	peer.paused.Store(true)
	if err := parallelsPause(ctx, peer.config.VM); err != nil {
		peer.paused.Store(false)
		return err
	}
	pauseStart := time.Now()
	if err := sleep(ctx, time.Second); err != nil {
		return err
	}
	stPre, err := state(ctx, host)
	if err != nil {
		return err
	}
	if !connected(stPre, peer.deviceID) {
		return fmt.Errorf("expected_half_open_connected_before_wake_probe")
	}

	tWake := time.Now()
	if err := host.action(ctx, "notify_network", nil); err != nil {
		return err
	}
	var transitioned engine.Connection
	detectDeadline := time.Now().Add(15 * time.Second)
	for time.Now().Before(detectDeadline) {
		st, err := state(ctx, host)
		if err != nil {
			return err
		}
		b, ok := bindingForPeer(st, peer.deviceID)
		if ok && b.Connection != engine.Connected {
			transitioned = b.Connection
			break
		}
		if err := sleep(ctx, 200*time.Millisecond); err != nil {
			return err
		}
	}
	if transitioned == "" {
		return fmt.Errorf("half_open_session_not_detected_within_budget")
	}
	detectMS := float64(time.Since(tWake)) / float64(time.Millisecond)
	if err := c.recordWakeCheck(wakeCheck{
		Node: host.name, Peer: peer.name, Case: "half_open_zombie_detection",
		Result: "passed", Milliseconds: detectMS,
		Detail: fmt.Sprintf("connected -> %s in %.0fms (vs 30s body timeout)", transitioned, detectMS),
	}); err != nil {
		return err
	}

	// Scenario B: Exceed 25s freeze (netmon wall-clock jump threshold) -> resume -> auto-reconnect + refresh + first copy.
	if remain := 26*time.Second - time.Since(pauseStart); remain > 0 {
		if err := sleep(ctx, remain); err != nil {
			return err
		}
	}
	tResume := time.Now()
	if err := resumePeer(); err != nil {
		return err
	}
	if err := waitEdge(ctx, host, peer, true); err != nil {
		return fmt.Errorf("post_resume_reconnect_failed: %w", err)
	}
	reconnMS := float64(time.Since(tResume)) / float64(time.Millisecond)

	var stRefreshed api.Status
	if err := host.call(ctx, "refresh", peer.deviceID, &stRefreshed); err != nil {
		return err
	}
	bPost, ok := bindingForPeer(stRefreshed, peer.deviceID)
	if !ok || bPost.Connection != engine.Connected || !validObservedPath(bPost.ConnectionPath) {
		return fmt.Errorf("post_resume_refresh_invalid")
	}
	if err := c.recordWakeCheck(wakeCheck{
		Node: host.name, Peer: peer.name, Case: "post_freeze_auto_reconnect",
		Result: "passed", Milliseconds: reconnMS, Path: bPost.ConnectionPath, LatencyMS: bPost.LatencyMs,
		Detail: fmt.Sprintf("reconnected=%.0fms path=%s rtt=%dms", reconnMS, bPost.ConnectionPath, bPost.LatencyMs),
	}); err != nil {
		return err
	}

	outResume, msResume, err := c.wakeTransfer(ctx, "post-resume-first-copy", host, peer)
	if err != nil {
		return err
	}
	if err := c.recordWakeCheck(wakeCheck{
		Node: host.name, Peer: peer.name, Case: "post_resume_first_copy",
		Result: "passed", Milliseconds: msResume,
		Detail: fmt.Sprintf("send_op=%.1fms verified=%.1fms", outResume.SendOperationMS, msResume),
	}); err != nil {
		return err
	}

	// Scenario C: Brief 5s freeze -> resume -> host wake warmup keeps healthy session alive -> immediate copy.
	stBeforeC, err := state(ctx, host)
	if err != nil {
		return err
	}
	bBeforeC, _ := bindingForPeer(stBeforeC, peer.deviceID)
	peer.paused.Store(true)
	if err := parallelsPause(ctx, peer.config.VM); err != nil {
		peer.paused.Store(false)
		return err
	}
	if err := sleep(ctx, 5*time.Second); err != nil {
		return err
	}
	if err := resumePeer(); err != nil {
		return err
	}
	if err := sleep(ctx, 500*time.Millisecond); err != nil {
		return err
	}

	tWarm := time.Now()
	if err := host.action(ctx, "notify_network", nil); err != nil {
		return err
	}
	var bWarmC engine.DeviceBinding
	if err := poll(ctx, "post_brief_freeze_warmup", func() (bool, error) {
		st, err := state(ctx, host)
		if err != nil {
			return false, err
		}
		b, ok := bindingForPeer(st, peer.deviceID)
		if ok && b.Connection == engine.Connected && b.LastCheckedMs > bBeforeC.LastCheckedMs {
			bWarmC = b
			return true, nil
		}
		return false, nil
	}); err != nil {
		return err
	}
	warmCMS := float64(time.Since(tWarm)) / float64(time.Millisecond)
	warmPath := bWarmC.ConnectionPath
	if warmPath == "" {
		warmPath = "passive"
	}
	if err := c.recordWakeCheck(wakeCheck{
		Node: host.name, Peer: peer.name, Case: "brief_freeze_wake_warmup",
		Result: "passed", Milliseconds: warmCMS, Path: bWarmC.ConnectionPath, LatencyMS: bWarmC.LatencyMs,
		Detail: fmt.Sprintf("warmed without drop path=%s rtt=%dms", warmPath, bWarmC.LatencyMs),
	}); err != nil {
		return err
	}

	outWarm, msWarm, err := c.wakeTransfer(ctx, "post-warmup-first-copy", host, peer)
	if err != nil {
		return err
	}
	if err := c.recordWakeCheck(wakeCheck{
		Node: host.name, Peer: peer.name, Case: "post_warmup_first_copy",
		Result: "passed", Milliseconds: msWarm,
		Detail: fmt.Sprintf("send_op=%.1fms verified=%.1fms", outWarm.SendOperationMS, msWarm),
	}); err != nil {
		return err
	}

	return c.check("wake-refresh: half-open zombie detection, post-freeze auto-recovery, wake warmup and first-copy transfers passed")
}
