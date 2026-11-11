package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"math"
	"reflect"
	"sort"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func sleep(ctx context.Context, d time.Duration) error {
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-time.After(d):
		return nil
	}
}
func poll(ctx context.Context, what string, fn func() (bool, error)) error {
	deadline := time.Now().Add(90 * time.Second)
	for time.Now().Before(deadline) {
		ok, err := fn()
		if err != nil {
			return err
		}
		if ok {
			return nil
		}
		if err = sleep(ctx, 100*time.Millisecond); err != nil {
			return err
		}
	}
	return fmt.Errorf("timeout_%s", what)
}
func state(ctx context.Context, n *node) (api.Status, error) {
	var out api.Status
	err := n.call(ctx, "state", nil, &out)
	return out, err
}
func connected(s api.Status, id string) bool {
	for _, p := range s.Peers {
		if p.ID == id {
			return p.Connected
		}
	}
	return false
}
func trusted(s api.Status, id string) bool {
	for _, p := range s.Peers {
		if p.ID == id {
			return true
		}
	}
	return false
}
func fingerprint(s api.Status, id string) string {
	for _, p := range s.Pending {
		if p.ID == id {
			return p.Fingerprint
		}
	}
	return ""
}
func (c *controller) activate(ctx context.Context, active []*node) error {
	selected := map[*node]bool{}
	for _, n := range active {
		selected[n] = true
	}
	for _, n := range c.nodes {
		if !selected[n] && n.running {
			if err := n.stop(ctx); err != nil {
				return err
			}
			n.running = false
		}
	}
	for _, n := range active {
		if !n.running {
			if err := n.action(ctx, "start", nil); err != nil {
				return err
			}
			n.running = true
		}
		s, err := state(ctx, n)
		if err != nil {
			return err
		}
		if n.deviceID != "" && n.deviceID != s.ID {
			return fmt.Errorf("%s: identity_changed", n.name)
		}
		n.deviceID = s.ID
	}
	return nil
}
func ensurePair(ctx context.Context, a, b *node, beforeApproval func() error) error {
	sa, err := state(ctx, a)
	if err != nil {
		return err
	}
	sb, err := state(ctx, b)
	if err != nil {
		return err
	}
	if trusted(sa, b.deviceID) && trusted(sb, a.deviceID) {
		if beforeApproval != nil {
			return fmt.Errorf("fresh_pair_required")
		}
		return waitEdge(ctx, a, b, true)
	}
	ticket, err := a.cli(ctx, "device", "addr")
	if err != nil {
		return err
	}
	if _, err = b.cli(ctx, "device", "add", string(bytes.TrimSpace(ticket.Out))); err != nil {
		return err
	}
	var fa, fb string
	if err = poll(ctx, "pairing_requests", func() (bool, error) {
		x, err := state(ctx, a)
		if err != nil {
			return false, err
		}
		y, err := state(ctx, b)
		if err != nil {
			return false, err
		}
		fa, fb = fingerprint(x, b.deviceID), fingerprint(y, a.deviceID)
		return fa != "" && fb != "", nil
	}); err != nil {
		return err
	}
	if fa != fb {
		return fmt.Errorf("pairing_fingerprint_mismatch")
	}
	if beforeApproval != nil {
		if err = beforeApproval(); err != nil {
			return err
		}
	}
	if _, err = a.cli(ctx, "device", "approve", b.deviceID, fb); err != nil {
		return err
	}
	if _, err = b.cli(ctx, "device", "approve", a.deviceID, fa); err != nil {
		return err
	}
	return waitEdge(ctx, a, b, true)
}
func waitEdge(ctx context.Context, a, b *node, online bool) error {
	return poll(ctx, "peer_connection", func() (bool, error) {
		x, err := state(ctx, a)
		if err != nil {
			return false, err
		}
		y, err := state(ctx, b)
		if err != nil {
			return false, err
		}
		return connected(x, b.deviceID) == online && connected(y, a.deviceID) == online, nil
	})
}
func (c *controller) pairAll(ctx context.Context, nodes []*node) error {
	for i, a := range nodes {
		for _, b := range nodes[i+1:] {
			if err := ensurePair(ctx, a, b, nil); err != nil {
				return err
			}
			fmt.Printf("PAIR %s %s\n", a.name, b.name)
		}
	}
	return nil
}
func (c *controller) prepare(ctx context.Context, n *node, p payload, seed string) (fixture, error) {
	var f fixture
	err := n.call(ctx, "prepare", prepareRequest{Payload: p, Seed: c.manifest.RunID + "/" + seed}, &f)
	return f, err
}
func arm(ctx context.Context, receiver, source *node, f fixture) error {
	return receiver.action(ctx, "arm", armRequest{Source: source.deviceID, Fixture: f})
}
func audit(ctx context.Context, n *node, expected int) error {
	var count int
	if err := n.call(ctx, "audit", nil, &count); err != nil {
		return err
	}
	if count != expected {
		return fmt.Errorf("%s: receive_count_%d_expected_%d", n.name, count, expected)
	}
	return nil
}
func submit(ctx context.Context, n *node, f fixture, key string) (submitResult, error) {
	var out submitResult
	err := n.call(ctx, "submit", submitRequest{Fixture: f.ID, Key: key}, &out)
	return out, err
}
func calibrate(ctx context.Context, n *node) (clockEstimate, error) {
	best := clockEstimate{UncertaintyMS: math.MaxFloat64}
	for i := 0; i < 12; i++ {
		before := time.Now()
		var remote int64
		if err := n.call(ctx, "clock", nil, &remote); err != nil {
			return best, err
		}
		elapsed := float64(time.Since(before)) / float64(time.Millisecond)
		current := clockEstimate{OffsetMS: float64(remote-before.UnixNano())/1e6 - elapsed/2, UncertaintyMS: elapsed / 2, ControllerMS: float64(before.UnixNano())/1e6 + elapsed/2}
		if current.UncertaintyMS < best.UncertaintyMS {
			best = current
		}
	}
	return best, nil
}
func (c *controller) clocks(ctx context.Context, phase, cell, position string, nodes []*node) (map[string]clockEstimate, error) {
	type item struct {
		name  string
		clock clockEstimate
		err   error
	}
	ch := make(chan item, len(nodes))
	for _, n := range nodes {
		go func(n *node) { v, err := calibrate(ctx, n); ch <- item{n.name, v, err} }(n)
	}
	result := map[string]clockEstimate{}
	var first error
	for range nodes {
		x := <-ch
		if x.err != nil {
			if first == nil {
				first = x.err
			}
			continue
		}
		result[x.name] = x.clock
	}
	if first != nil {
		return nil, first
	}
	for _, n := range nodes {
		c.manifest.Calibrations = append(c.manifest.Calibrations, calibration{Scenario: phase, Cell: cell, Node: n.name, Position: position, Clock: result[n.name]})
	}
	return result, nil
}
func (c *controller) transfer(ctx context.Context, phase string, source *node, receivers []*node, f fixture, key string, clocks map[string]clockEstimate, round int, nominal int64) (submitResult, error) {
	for _, n := range receivers {
		if err := arm(ctx, n, source, f); err != nil {
			return submitResult{}, err
		}
	}
	before := time.Now()
	out, err := submit(ctx, source, f, key)
	submitted := time.Since(before)
	if err != nil {
		return out, err
	}
	if out.Result.Phase != engine.SubmissionCommitted || out.Result.HistoryID == "" {
		return out, fmt.Errorf("local_commit_missing")
	}
	if len(receivers) > 0 && out.Result.SyncDisposition != engine.SyncQueued {
		return out, fmt.Errorf("dispatch_not_queued")
	}
	if len(receivers) == 0 && out.Result.SyncDisposition != engine.SyncNoTargets {
		return out, fmt.Errorf("offline_dispatch_was_queued")
	}
	type arrived struct {
		node     *node
		delivery delivery
		duration time.Duration
		err      error
	}
	ch := make(chan arrived, len(receivers))
	for _, n := range receivers {
		go func(n *node) {
			var d delivery
			err := n.call(ctx, "wait", waitRequest{TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &d)
			ch <- arrived{n, d, time.Since(before), err}
		}(n)
	}
	results := map[string]arrived{}
	var first error
	for range receivers {
		x := <-ch
		if x.err != nil && first == nil {
			first = x.err
		}
		results[x.node.name] = x
	}
	for _, n := range receivers {
		x := results[n.name]
		event := targetOutcome{Scenario: phase, Case: key, Sender: source.name, Receiver: n.name, Kind: f.Kind, Bytes: f.Total, Expected: f.Parts, Actual: x.delivery.Actual, HashVerified: x.err == nil, VerifiedMS: float64(x.duration) / float64(time.Millisecond)}
		if x.err != nil {
			event.Error = x.err.Error()
		}
		c.manifest.Outcomes = append(c.manifest.Outcomes, event)
	}
	if err = c.save(); err != nil {
		return out, err
	}
	if first != nil {
		return out, first
	}
	if err = sleep(ctx, 200*time.Millisecond); err != nil {
		return out, err
	}
	for _, n := range receivers {
		if err = audit(ctx, n, 1); err != nil {
			return out, err
		}
	}
	if clocks != nil {
		fanout := 0.0
		added := []sample{}
		for _, n := range receivers {
			x := results[n.name]
			sc, rc := clocks[source.name], clocks[n.name]
			estimate := float64(x.delivery.ReceivedMS) - float64(out.StartedNS)/1e6 + sc.OffsetMS - rc.OffsetMS
			uncertainty := sc.UncertaintyMS + rc.UncertaintyMS + 1
			fanout = math.Max(fanout, estimate)
			added = append(added, sample{Scenario: phase, Case: key, Sender: source.name, Receiver: n.name, Kind: f.Kind, NominalSize: nominal, Bytes: f.Total, Round: round, Entry: out.Entry, Parts: f.Parts, HashVerified: true, ReceivedParts: x.delivery.Actual, ReceiptID: x.delivery.Receipt, HistoryID: x.delivery.History, Count: x.delivery.Count, SubmitNS: out.StartedNS, ReceiveMS: x.delivery.ReceivedMS, SubmitMS: float64(submitted) / float64(time.Millisecond), VerifiedMS: float64(x.duration) / float64(time.Millisecond), EstimateMS: estimate, UncertaintyMS: uncertainty, LowerMS: math.Max(0, estimate-uncertainty), UpperMS: math.Max(0, estimate+uncertainty), SenderOffsetMS: sc.OffsetMS, ReceiverOffsetMS: rc.OffsetMS, Path: "unknown"})
		}
		for i := range added {
			added[i].FanoutLastMS = fanout
		}
		c.manifest.Samples = append(c.manifest.Samples, added...)
		if err = c.save(); err != nil {
			return out, err
		}
	}
	return out, nil
}
func (c *controller) matrix(ctx context.Context, phase string, nodes []*node) error {
	for _, p := range payloads(c.config.Suite) {
		cell := fmt.Sprintf("%s-%d", p.Kind, p.Size)
		clocks, err := c.clocks(ctx, phase, cell, "before", nodes)
		if err != nil {
			return err
		}
		begin := len(c.manifest.Samples)
		for round := 1; round <= c.config.Suite.Repetitions; round++ {
			for _, source := range nodes {
				key := digest([]byte(fmt.Sprintf("%s/%s/%s/%s/%d", c.manifest.RunID, phase, cell, source.name, round)))[:32]
				f, err := c.prepare(ctx, source, p, key)
				if err != nil {
					return err
				}
				receivers := []*node{}
				for _, n := range nodes {
					if n != source {
						receivers = append(receivers, n)
					}
				}
				if _, err = c.transfer(ctx, phase, source, receivers, f, key, clocks, round, p.Size); err != nil {
					return err
				}
				if err = source.action(ctx, "release", f.ID); err != nil {
					return err
				}
				fmt.Printf("SAMPLE %s %s %s %d round=%d receivers=%d\n", phase, source.name, p.Kind, p.Size, round, len(receivers))
			}
		}
		after, err := c.clocks(ctx, phase, cell, "after", nodes)
		if err != nil {
			return err
		}
		for i := begin; i < len(c.manifest.Samples); i++ {
			s := &c.manifest.Samples[i]
			s.UncertaintyMS += math.Abs(after[s.Sender].OffsetMS-clocks[s.Sender].OffsetMS) + math.Abs(after[s.Receiver].OffsetMS-clocks[s.Receiver].OffsetMS)
			s.LowerMS = math.Max(0, s.EstimateMS-s.UncertaintyMS)
			s.UpperMS = math.Max(0, s.EstimateMS+s.UncertaintyMS)
			if s.EstimateMS < -s.UncertaintyMS {
				return fmt.Errorf("receive_timestamp_outside_clock_bound")
			}
		}
		if err = c.save(); err != nil {
			return err
		}
	}
	return c.check(phase + ": every expected receiver exported matching parts exactly once")
}
func historyOf(ctx context.Context, n *node) (historySnapshot, error) {
	var out historySnapshot
	err := n.call(ctx, "history", nil, &out)
	return out, err
}
func historyIDs(v historySnapshot) []string {
	out := []string{}
	for _, it := range v.Items {
		out = append(out, it.ID)
	}
	sort.Strings(out)
	return out
}
func detail(ctx context.Context, n *node, id string) (api.HistoryItem, error) {
	v, err := n.cli(ctx, "history", "show", id, "--json")
	if err != nil {
		return api.HistoryItem{}, err
	}
	var out api.HistoryItem
	err = json.Unmarshal(v.Out, &out)
	return out, err
}
func hasTag(v api.HistoryItem, tag string) bool {
	for _, t := range v.Tags {
		if t == tag {
			return true
		}
	}
	return false
}
func (c *controller) contracts(ctx context.Context) error {
	a, b := c.nodes[0], c.nodes[1]
	if err := c.activate(ctx, []*node{a, b}); err != nil {
		return err
	}
	f, err := c.prepare(ctx, a, payload{"text", 32}, "no-target")
	if err != nil {
		return err
	}
	key := digest([]byte(c.manifest.RunID + "no-target"))[:32]
	first, err := submit(ctx, a, f, key)
	if err != nil {
		return err
	}
	if first.Result.Phase != engine.SubmissionCommitted || first.Result.SyncDisposition != engine.SyncNoTargets || first.Result.HistoryID == "" {
		return fmt.Errorf("no_target_local_commit_failed")
	}
	again, err := submit(ctx, a, f, key)
	if err != nil || !reflect.DeepEqual(first.Result, again.Result) {
		return fmt.Errorf("same_key_result_changed")
	}
	var queried cliResult
	if err = a.call(ctx, "cli", cliRequest{Args: []string{"dev", "submission", key}}, &queried); err != nil {
		return err
	}
	var original engine.SubmissionResult
	if err = json.Unmarshal(queried.Out, &original); err != nil || !reflect.DeepEqual(original, first.Result) {
		return fmt.Errorf("submission_query_mismatch")
	}
	var conflict cliResult
	if err = a.call(ctx, "cli", cliRequest{Args: []string{"send", "--to", "all", "--json", "--request-id", key, "conflicting-input"}, AllowFailure: true}, &conflict); err != nil {
		return err
	}
	if conflict.Code == 0 || !bytes.Contains(conflict.Err, []byte(`"reason":"conflict"`)) {
		return fmt.Errorf("same_key_conflict_not_rejected")
	}
	if err = c.check("contract: no-target local commit, same-key result/query and conflicting-input rejection"); err != nil {
		return err
	}
	pending, err := c.prepare(ctx, a, payload{"text", 48}, "pending-approval")
	if err != nil {
		return err
	}
	err = ensurePair(ctx, a, b, func() error {
		if err := arm(ctx, b, a, pending); err != nil {
			return err
		}
		out, err := submit(ctx, a, pending, digest([]byte(c.manifest.RunID + "pending"))[:32])
		if err != nil {
			return err
		}
		if out.Result.Phase != engine.SubmissionCommitted {
			return fmt.Errorf("pending_local_commit_failed")
		}
		if err = sleep(ctx, time.Second); err != nil {
			return err
		}
		return audit(ctx, b, 0)
	})
	if err != nil {
		return err
	}
	if err = c.check("contract: matching fingerprints, no pre-approval delivery and mutual explicit approval"); err != nil {
		return err
	}
	mail, err := c.prepare(ctx, a, payload{"mail", 32}, "receive-tag")
	if err != nil {
		return err
	}
	if err = arm(ctx, b, a, mail); err != nil {
		return err
	}
	requestKey := digest([]byte(c.manifest.RunID + "concurrent"))[:32]
	var retries []engine.SubmissionResult
	if err = a.call(ctx, "retry", submitRequest{Fixture: mail.ID, Key: requestKey}, &retries); err != nil {
		return err
	}
	if len(retries) != 4 {
		return fmt.Errorf("missing_concurrent_results")
	}
	for _, r := range retries {
		if r.Phase != engine.SubmissionCommitted || !reflect.DeepEqual(r, retries[0]) {
			return fmt.Errorf("concurrent_retry_not_idempotent")
		}
	}
	var rx delivery
	if err = b.call(ctx, "wait", waitRequest{TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &rx); err != nil {
		return err
	}
	if err = sleep(ctx, time.Second); err != nil {
		return err
	}
	if err = audit(ctx, b, 1); err != nil {
		return err
	}
	sourceItem, err := detail(ctx, a, string(retries[0].HistoryID))
	if err != nil {
		return err
	}
	targetItem, err := detail(ctx, b, rx.History)
	if err != nil {
		return err
	}
	if !hasTag(sourceItem, "local-"+a.name) || hasTag(sourceItem, "local-"+b.name) || !hasTag(targetItem, "local-"+b.name) || hasTag(targetItem, "local-"+a.name) {
		return fmt.Errorf("processor_tags_not_local")
	}
	if err = c.check("contract: four concurrent production CLI retries deliver once; receiver annotations remain local"); err != nil {
		return err
	}
	sensitive, err := c.prepare(ctx, a, payload{"sensitive", 32}, "sensitive")
	if err != nil {
		return err
	}
	if err = arm(ctx, b, a, sensitive); err != nil {
		return err
	}
	blocked, err := submit(ctx, a, sensitive, digest([]byte(c.manifest.RunID + "sensitive"))[:32])
	if err != nil {
		return err
	}
	it, err := detail(ctx, a, string(blocked.Result.HistoryID))
	if err != nil {
		return err
	}
	if blocked.Result.Phase != engine.SubmissionCommitted || blocked.Result.SyncDisposition == engine.SyncQueued || !hasTag(it, "sensitive") || it.ExpiresAt == 0 {
		return fmt.Errorf("sensitive_policy_failed")
	}
	if err = sleep(ctx, time.Second); err != nil {
		return err
	}
	if err = audit(ctx, b, 0); err != nil {
		return err
	}
	if err = c.check("contract: sensitive processor preserves local history/tags/expiry and blocks sending"); err != nil {
		return err
	}
	for _, policy := range []struct {
		key  string
		want engine.SyncDisposition
	}{{"enabled", engine.SyncDisabled}, {"text", engine.SyncPolicyFilter}} {
		if err = a.action(ctx, "policy", map[string]bool{policy.key: false}); err != nil {
			return err
		}
		p, err := c.prepare(ctx, a, payload{"text", 40}, "policy-"+policy.key)
		if err != nil {
			return err
		}
		if err = arm(ctx, b, a, p); err != nil {
			return err
		}
		out, err := submit(ctx, a, p, digest([]byte(c.manifest.RunID + policy.key))[:32])
		if err != nil {
			return err
		}
		if out.Result.Phase != engine.SubmissionCommitted || out.Result.HistoryID == "" || out.Result.SyncDisposition != policy.want {
			return fmt.Errorf("blocked_sync_prevented_local_commit")
		}
		if err = sleep(ctx, time.Second); err != nil {
			return err
		}
		if err = audit(ctx, b, 0); err != nil {
			return err
		}
		if err = a.action(ctx, "policy", map[string]bool{policy.key: true}); err != nil {
			return err
		}
		if err = sleep(ctx, time.Second); err != nil {
			return err
		}
		if err = audit(ctx, b, 0); err != nil {
			return err
		}
	}
	return c.check("contract: global disable and text filter preserve local commit; enable does not replay blocked content")
}
func requireExistingMesh(ctx context.Context, nodes []*node) error {
	for i, a := range nodes {
		for _, b := range nodes[i+1:] {
			sa, err := state(ctx, a)
			if err != nil {
				return err
			}
			sb, err := state(ctx, b)
			if err != nil {
				return err
			}
			if !trusted(sa, b.deviceID) || !trusted(sb, a.deviceID) {
				return fmt.Errorf("trust_missing_after_restart")
			}
			if err = waitEdge(ctx, a, b, true); err != nil {
				return err
			}
		}
	}
	return nil
}
func (c *controller) isolation(ctx context.Context) error {
	if err := c.activate(ctx, c.nodes); err != nil {
		return err
	}
	if err := c.pairAll(ctx, c.nodes); err != nil {
		return err
	}
	for _, sender := range c.nodes {
		f, err := c.prepare(ctx, sender, payload{"text", 64}, "restart-baseline-"+sender.name)
		if err != nil {
			return err
		}
		receivers := []*node{}
		for _, receiver := range c.nodes {
			if receiver != sender {
				receivers = append(receivers, receiver)
			}
		}
		if _, err = c.transfer(ctx, "isolation", sender, receivers, f, digest([]byte(c.manifest.RunID + "restart-baseline-" + sender.name))[:32], nil, 0, 64); err != nil {
			return err
		}
	}
	snapshots := map[*node]historySnapshot{}
	for _, n := range c.nodes {
		h, err := historyOf(ctx, n)
		if err != nil {
			return err
		}
		snapshots[n] = h
	}
	for _, n := range c.nodes {
		if err := n.stop(ctx); err != nil {
			return err
		}
		n.running = false
	}
	if err := c.activate(ctx, c.nodes); err != nil {
		return err
	}
	for _, n := range c.nodes {
		h, err := historyOf(ctx, n)
		if err != nil {
			return err
		}
		if !reflect.DeepEqual(historyIDs(h), historyIDs(snapshots[n])) {
			return fmt.Errorf("%s: history_ids_changed_on_restart", n.name)
		}
	}
	if err := requireExistingMesh(ctx, c.nodes); err != nil {
		return err
	}
	if err := c.check("isolation: all node identities, trust and history IDs survive service restart"); err != nil {
		return err
	}
	source, victim := c.nodes[0], c.nodes[len(c.nodes)-1]
	before, err := historyOf(ctx, victim)
	if err != nil {
		return err
	}
	if err = victim.stop(ctx); err != nil {
		return err
	}
	victim.running = false
	if err = poll(ctx, "offline_peer", func() (bool, error) {
		s, err := state(ctx, source)
		if err != nil {
			return false, err
		}
		return !connected(s, victim.deviceID), nil
	}); err != nil {
		return err
	}
	receivers := []*node{}
	for _, n := range c.nodes {
		if n != source && n != victim {
			receivers = append(receivers, n)
		}
	}
	f, err := c.prepare(ctx, source, payload{"text", 96}, "offline-probe")
	if err != nil {
		return err
	}
	if _, err = c.transfer(ctx, "isolation", source, receivers, f, digest([]byte(c.manifest.RunID + "offline"))[:32], nil, 0, 96); err != nil {
		return err
	}
	if err = c.activate(ctx, c.nodes); err != nil {
		return err
	}
	if err = requireExistingMesh(ctx, c.nodes); err != nil {
		return err
	}
	if err = sleep(ctx, time.Second); err != nil {
		return err
	}
	after, err := historyOf(ctx, victim)
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(historyIDs(before), historyIDs(after)) {
		return fmt.Errorf("offline_content_replayed")
	}
	allOthers := []*node{}
	for _, n := range c.nodes {
		if n != source {
			allOthers = append(allOthers, n)
		}
	}
	f, err = c.prepare(ctx, source, payload{"text", 112}, "after-recovery")
	if err != nil {
		return err
	}
	if _, err = c.transfer(ctx, "isolation", source, allOthers, f, digest([]byte(c.manifest.RunID + "recovery"))[:32], nil, 0, 112); err != nil {
		return err
	}
	if err = c.check("isolation: stopping one node preserves surviving-node sync; reconnect does not replay offline content and accepts new content"); err != nil {
		return err
	}
	if _, err = victim.cli(ctx, "device", "remove", source.deviceID); err != nil {
		return err
	}
	if err = waitEdge(ctx, source, victim, false); err != nil {
		return err
	}
	before, err = historyOf(ctx, victim)
	if err != nil {
		return err
	}
	f, err = c.prepare(ctx, source, payload{"text", 128}, "revoked-probe")
	if err != nil {
		return err
	}
	if _, err = c.transfer(ctx, "isolation", source, receivers, f, digest([]byte(c.manifest.RunID + "revoked"))[:32], nil, 0, 128); err != nil {
		return err
	}
	if err = sleep(ctx, time.Second); err != nil {
		return err
	}
	after, err = historyOf(ctx, victim)
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(historyIDs(before), historyIDs(after)) {
		return fmt.Errorf("revoked_node_received_content")
	}
	return c.check("isolation: revocation blocks its target without breaking surviving nodes or local commits")
}
func (c *controller) runScenarios(ctx context.Context) error {
	enabled := map[string]bool{}
	for _, v := range c.config.Suite.Scenarios {
		enabled[v] = true
	}
	if enabled["contract"] {
		if err := c.contracts(ctx); err != nil {
			return err
		}
	}
	if enabled["pairwise"] && !(len(c.nodes) == 2 && enabled["mesh"]) {
		for i, a := range c.nodes {
			for _, b := range c.nodes[i+1:] {
				active := []*node{a, b}
				if err := c.activate(ctx, active); err != nil {
					return err
				}
				if err := ensurePair(ctx, a, b, nil); err != nil {
					return err
				}
				if err := c.matrix(ctx, "pairwise-"+a.name+"-"+b.name, active); err != nil {
					return err
				}
			}
		}
	}
	if enabled["mesh"] {
		if err := c.activate(ctx, c.nodes); err != nil {
			return err
		}
		if err := c.pairAll(ctx, c.nodes); err != nil {
			return err
		}
		if err := c.matrix(ctx, "mesh", c.nodes); err != nil {
			return err
		}
		if len(c.nodes) == 2 && enabled["pairwise"] {
			if err := c.check("pairwise: two-node mesh also supplies the isolated pairwise baseline"); err != nil {
				return err
			}
		}
	}
	if enabled["isolation"] {
		return c.isolation(ctx)
	}
	return nil
}
