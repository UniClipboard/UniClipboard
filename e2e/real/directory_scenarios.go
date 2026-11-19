package main

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/uniclipboard/unic/engine"
)

func directoryCall(ctx context.Context, n *node, v directoryCommand, out any) error {
	return n.call(ctx, "directory", v, out)
}
func (c *controller) directoryRecord(node, name, result, detail string, ms float64, resources *directoryResources) error {
	c.manifest.DirectoryChecks = append(c.manifest.DirectoryChecks, directoryCheck{Case: name, Node: node, Result: result, Detail: detail, Milliseconds: ms, Resources: resources})
	if err := c.save(); err != nil {
		return err
	}
	fmt.Printf("%s directory %s %s\n", result, node, name)
	return nil
}
func directoryReleased(ctx context.Context, n *node) (directoryResources, error) {
	var resources directoryResources
	end := time.Now().Add(45 * time.Second)
	for time.Now().Before(end) {
		resources = directoryResources{}
		if err := directoryCall(ctx, n, directoryCommand{Action: "resources"}, &resources); err != nil {
			return resources, err
		}
		if resources.IncomingFiles == 0 && resources.TransientDirectories == 0 && resources.StageOpenFiles <= 0 {
			return resources, nil
		}
		if err := sleep(ctx, 50*time.Millisecond); err != nil {
			return resources, err
		}
	}
	data, _ := json.Marshal(resources)
	return resources, fmt.Errorf("%s_directory_resources_not_released: %s", n.name, data)
}
func directoryAbsent(ctx context.Context, n *node) error {
	end := time.Now().Add(2 * time.Second)
	for time.Now().Before(end) {
		if err := audit(ctx, n, 0); err != nil {
			return err
		}
		if err := sleep(ctx, 100*time.Millisecond); err != nil {
			return err
		}
	}
	return nil
}
func (c *controller) directoryFailures(ctx context.Context, focus string) error {
	if len(c.nodes) != 2 {
		return fmt.Errorf("directory_fault_catalog_requires_two_nodes")
	}
	if err := c.activate(ctx, c.nodes); err != nil {
		return err
	}
	if err := c.pairAll(ctx, c.nodes); err != nil {
		return err
	}
	for i, source := range c.nodes {
		receiver := c.nodes[1-i]

		for _, phase := range []string{"cancel-scanning", "cancel-preparing", "mutate-preparing", "respond"} {
			if focus != "" {
				continue
			}
			f, err := c.prepare(ctx, source, payload{"directory", 1024}, source.name+"-"+phase)
			if err != nil {
				return err
			}
			if err = arm(ctx, receiver, source, f); err != nil {
				return err
			}
			command := directoryCommand{Action: phase, Fixture: f.ID, Key: source.name + "-" + phase}
			var text fixture
			if phase == "respond" {
				text, err = c.prepare(ctx, source, payload{"text", 64}, source.name+"-concurrent-text")
				if err != nil {
					return err
				}
				command.TextFixture = text.ID
				if err = arm(ctx, receiver, source, text); err != nil {
					return err
				}
			}
			var elapsed float64
			var reason string
			if phase == "mutate-preparing" {
				err = directoryCall(ctx, source, command, &reason)
			} else {
				err = directoryCall(ctx, source, command, &elapsed)
			}
			if err != nil {
				return fmt.Errorf("%s_%s: %w", source.name, phase, err)
			}
			if phase == "respond" {
				if elapsed > 5000 {
					return fmt.Errorf("directory_concurrent_text_exceeded_5s_budget")
				}
				var d delivery
				if err = receiver.call(ctx, "wait", waitRequest{TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &d); err != nil {
					return err
				}
				// Explicit directory sends remain independent; terminate this task-owned send after the text proof.
				var code int
				if err = directoryCall(ctx, source, directoryCommand{Action: "await"}, &code); err != nil {
					return err
				}
				if code != 0 {
					return fmt.Errorf("concurrent_directory_send_failed")
				}
				// Keep the original pre-send baseline while switching back from the text oracle.
				if err = receiver.action(ctx, "rearm", armRequest{Source: source.deviceID, Fixture: f}); err != nil {
					return err
				}
				if err = receiver.call(ctx, "wait", waitRequest{TimeoutSeconds: c.config.Suite.ReceiveTimeoutSeconds}, &d); err != nil {
					return err
				}

				if err = source.action(ctx, "release", text.ID); err != nil {
					return err
				}
			} else {
				if phase != "mutate-preparing" && elapsed > 5000 {
					return fmt.Errorf("directory_preparation_cancel_exceeded_5s_budget")
				}
				if err = directoryAbsent(ctx, receiver); err != nil {
					return err
				}
			}
			r, err := directoryReleased(ctx, source)
			if err != nil {
				return err
			}
			if err = c.directoryRecord(source.name, phase, "PASS", reason, elapsed, &r); err != nil {
				return err
			}
			if err = source.action(ctx, "release", f.ID); err != nil {
				return err
			}
		}
		// Independent malformed source selections use the same real two-host path.
		for _, fault := range []string{"depth", "entries", "files", "symlink", "remove-source", "file-size"} {
			if focus != "" {
				continue
			}
			f, err := c.prepare(ctx, source, payload{"directory", 0}, "fault-"+source.name+"-"+fault)
			if err != nil {
				return err
			}
			if err = arm(ctx, receiver, source, f); err != nil {
				return err
			}
			if fault == "file-size" {
				if _, err = source.cli(ctx, "config", "set", "limits.max_file_bytes", "1"); err != nil {
					return err
				}
			} else if err = directoryCall(ctx, source, directoryCommand{Action: fault, Fixture: f.ID}, nil); err != nil {
				return err
			}
			var reason string
			err = directoryCall(ctx, source, directoryCommand{Action: "reject", Fixture: f.ID, Key: source.name + "-reject-" + fault}, &reason)
			if fault == "file-size" {
				if _, e := source.cli(ctx, "config", "set", "limits.max_file_bytes", `"100MB"`); err == nil {
					err = e
				}
			}
			if err != nil {
				return fmt.Errorf("%s_%s: %w", source.name, fault, err)
			}
			if err = directoryAbsent(ctx, receiver); err != nil {
				return err
			}
			r, err := directoryReleased(ctx, source)
			if err != nil {
				return err
			}
			if err = c.directoryRecord(source.name, fault, "PASS", reason, 0, &r); err != nil {
				return err
			}
			if err = source.action(ctx, "release", f.ID); err != nil {
				return err
			}
		}
		// A disconnected peer is excluded from admission; reconnect does not replay.
		if focus == "" {
			offline, err := c.prepare(ctx, source, payload{"directory", 0}, "offline-"+source.name)
			if err != nil {
				return err
			}
			if err = arm(ctx, receiver, source, offline); err != nil {
				return err
			}
			if err = receiver.stop(ctx); err != nil {
				return err
			}
			receiver.running = false
			if err = poll(ctx, "directory_peer_offline", func() (bool, error) { s, e := state(ctx, source); return !connected(s, receiver.deviceID), e }); err != nil {
				return err
			}
			out, err := submit(ctx, source, offline, source.name+"-offline-directory")
			if err != nil {
				return err
			}
			if out.Result.SyncDisposition != engine.SyncNoTargets {
				return fmt.Errorf("offline_directory_wrong_disposition")
			}
			if err = receiver.action(ctx, "start", nil); err != nil {
				return err
			}
			receiver.running = true
			if err = waitEdge(ctx, source, receiver, true); err != nil {
				return err
			}
			if err = directoryAbsent(ctx, receiver); err != nil {
				return err
			}
			if err = c.directoryRecord(source.name, "offline-restart-no-replay", "PASS", "fresh explicit send required after reconnect", 0, nil); err != nil {
				return err
			}
			fresh, err := c.prepare(ctx, source, payload{"directory", 0}, "recovered-"+source.name)
			if err != nil {
				return err
			}
			if _, err = c.transfer(ctx, "directory-recovery", source, []*node{receiver}, fresh, source.name+"-fresh-directory", nil, 1, 0); err != nil {
				return err
			}
			if err = directoryCall(ctx, receiver, directoryCommand{Action: "export-ownership"}, nil); err != nil {
				return err
			}
			if err = waitEdge(ctx, source, receiver, true); err != nil {
				return err
			}
			if err = c.directoryRecord(receiver.name, "export-ownership-delete-restart", "PASS", "root collision rejected; user copies survived history deletion and service restart", 0, nil); err != nil {
				return err
			}
			for _, f := range []fixture{offline, fresh} {
				if err = source.action(ctx, "release", f.ID); err != nil {
					return err
				}
			}
		}
		// Cancel an active transfer after receiver staging contains payload bytes.
		for _, fault := range []string{"cancel", "receiver-stop", "revoke", "sender-crash"} {
			if focus != "" && focus != fault {
				continue
			}
			f, err := c.prepare(ctx, source, payload{"directory", 16 << 20}, source.name+"-active-"+fault)
			if err != nil {
				return err
			}
			if err = arm(ctx, receiver, source, f); err != nil {
				return err
			}
			key := source.name + "-active-" + fault
			if err = directoryCall(ctx, source, directoryCommand{Action: "begin", Fixture: f.ID, Key: key}, nil); err != nil {
				return err
			}
			if err = poll(ctx, "directory_receive_payload", func() (bool, error) {
				var r directoryResources
				e := directoryCall(ctx, receiver, directoryCommand{Action: "resources"}, &r)
				return r.IncomingBytes > 0, e
			}); err != nil {
				return err
			}
			started := time.Now()
			switch fault {
			case "cancel":
				var ms float64
				if err = directoryCall(ctx, source, directoryCommand{Action: "cancel"}, &ms); err != nil {
					return err
				}
				if ms > 5000 {
					return fmt.Errorf("directory_cancel_exceeded_5s_budget")
				}
			case "receiver-stop":
				if err = receiver.stop(ctx); err != nil {
					return err
				}
				receiver.running = false
				var code int
				if err = directoryCall(ctx, source, directoryCommand{Action: "await"}, &code); err != nil {
					return err
				}
				if code == 0 {
					return fmt.Errorf("interrupted_directory_reported_success")
				}
				if err = receiver.action(ctx, "start", nil); err != nil {
					return err
				}
				receiver.running = true
				if err = waitEdge(ctx, source, receiver, true); err != nil {
					return err
				}
			case "sender-crash":
				if err = directoryCall(ctx, source, directoryCommand{Action: "crash"}, nil); err != nil {
					return err
				}
				source.running = false
				var code int
				if err = directoryCall(ctx, source, directoryCommand{Action: "await"}, &code); err != nil {
					return err
				}
				if code == 0 {
					return fmt.Errorf("crashed_directory_reported_success")
				}
			case "revoke":
				if _, err = source.cli(ctx, "device", "remove", receiver.deviceID); err != nil {
					return err
				}
				var code int
				if err = directoryCall(ctx, source, directoryCommand{Action: "await"}, &code); err != nil {
					return err
				}
				if code == 0 {
					return fmt.Errorf("revoked_directory_reported_success")
				}
			}
			if err = directoryAbsent(ctx, receiver); err != nil {
				return err
			}
			r, err := directoryReleased(ctx, receiver)
			if err != nil {
				return err
			}
			if fault == "sender-crash" {
				if err = source.action(ctx, "start", nil); err != nil {
					return err
				}
				source.running = true
				if err = waitEdge(ctx, source, receiver, true); err != nil {
					return err
				}
			}
			if _, err = directoryReleased(ctx, source); err != nil {
				return err
			}
			if err = c.directoryRecord(source.name, fault, "PASS", "payload observed in receiver staging; no complete receipt; staging released", float64(time.Since(started))/float64(time.Millisecond), &r); err != nil {
				return err
			}
			if err = source.action(ctx, "release", f.ID); err != nil {
				return err
			}
			if fault == "revoke" {
				// Reset the other half of this test relationship before a fresh mutual approval.
				if _, err = receiver.cli(ctx, "device", "remove", source.deviceID); err != nil {
					return err
				}
				if err = ensurePair(ctx, source, receiver, nil); err != nil {
					return err
				}
			}
			recovered, err := c.prepare(ctx, source, payload{"directory", 0}, source.name+"-after-"+fault)
			if err != nil {
				return err
			}
			if _, err = c.transfer(ctx, "directory-after-"+fault, source, []*node{receiver}, recovered, source.name+"-after-"+fault, nil, 1, 0); err != nil {
				return err
			}
			if err = source.action(ctx, "release", recovered.ID); err != nil {
				return err
			}
		}
	}
	if focus != "" {
		return c.check("directory-" + focus + ": observed active payload, rejected partial publication, released resources and fresh recovery")
	}
	return c.check("directory: real-host bounds, offline recovery, cancellation, receive interruption, revocation and user export ownership")
}
