// Command real coordinates production CLI tests across local and SSH hosts.
package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"syscall"
	"time"
)

func main() {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	if err := mainCommand(ctx, os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func mainCommand(ctx context.Context, args []string) error {
	if len(args) == 0 {
		return fmt.Errorf("usage: unic-e2e doctor|plan|run --lab FILE [--out DIR] [--source DIR]")
	}
	fs := flag.NewFlagSet(args[0], flag.ContinueOnError)
	if args[0] == "worker" {
		runID := fs.String("run-id", "", "run identity")
		name := fs.String("node", "", "node label")
		if err := fs.Parse(args[1:]); err != nil {
			return err
		}
		return workerMain(ctx, *runID, *name)
	}
	if args[0] != "doctor" && args[0] != "run" && args[0] != "plan" {
		return fmt.Errorf("unknown harness command")
	}
	labFile := fs.String("lab", "", "node inventory")
	out := fs.String("out", "", "artifact directory (must not exist)")
	source := fs.String("source", ".", "repository root")
	suite := fs.String("suite", "sync", "suite name")
	allowShared := fs.Bool("allow-shared-hosts", false, "explicit controlled logical-node acceptance; not distinct-machine evidence")
	keepPrivate := fs.Bool("keep-private", false, "retain node profiles and controller private artifacts for diagnosis")
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}
	if fs.NArg() != 0 || *labFile == "" || *suite != "sync" {
		return fmt.Errorf("--lab is required; only suite sync is supported")
	}
	cfg, err := readLab(*labFile)
	if err != nil {
		return err
	}
	if args[0] != "doctor" && len(cfg.Nodes) < 2 {
		return fmt.Errorf("run and plan require at least two nodes")
	}
	names := labels(cfg)
	if args[0] == "plan" {
		var pairs [][2]string
		for i, a := range names {
			for _, b := range names[i+1:] {
				pairs = append(pairs, [2]string{a, b})
			}
		}
		return json.NewEncoder(os.Stdout).Encode(map[string]any{"nodes": names, "pairing_edges": pairs, "directed_links": len(names) * (len(names) - 1), "payload_cells": len(payloads(cfg.Suite)), "repetitions": cfg.Suite.Repetitions, "scenarios": cfg.Suite.Scenarios, "executes_hosts": false, "planned_measurements": plannedMeasurements(cfg), "payload_bytes_upper_bound": plannedPayloadBound(cfg), "payload_budget_excludes_setup_and_transport_overhead": true})
	}
	id := make([]byte, 16)
	if _, err = rand.Read(id); err != nil {
		return err
	}
	runID := hex.EncodeToString(id)
	if *out == "" {
		*out = filepath.Join(".cache", "real", runID)
	}
	absolute, err := filepath.Abs(*out)
	if err != nil {
		return err
	}
	if _, err = os.Stat(absolute); !os.IsNotExist(err) {
		return fmt.Errorf("output directory must not exist")
	}
	root, err := filepath.Abs(*source)
	if err != nil {
		return err
	}
	c := &controller{config: cfg, output: absolute, public: filepath.Join(absolute, "public"), private: filepath.Join(absolute, "private"), source: root}
	if err = os.MkdirAll(c.public, 0700); err != nil {
		return err
	}
	if err = os.MkdirAll(c.private, 0700); err != nil {
		return err
	}
	c.manifest = manifest{Version: 1, RunID: runID, PlannedMeasurements: plannedMeasurements(cfg), PayloadBound: plannedPayloadBound(cfg), Mode: args[0], Status: "running", Started: time.Now().UTC(), Suite: cfg.Suite, SharedHostControl: *allowShared, Limits: []string{"System clipboard, native rendering, sleep/wake and network switching are not exercised.", "HTML submission uses a test injector through the production local API; other content uses the production CLI.", "Distinct OS machine identities do not attest bare-metal hardware or exclude virtual machines.", "Network paths remain unknown unless observed independently; SSH reachability does not prove a direct synchronization path.", "Negative checks use bounded observation windows. Stops use the observed service generation; Unix SIGTERM or Windows termination are fallbacks. Forced stops are recorded.", "Three samples do not support a reliable tail-latency percentile. Inspect all raw samples and bounds."}}
	c.manifest.KeepPrivate = *keepPrivate
	if args[0] == "doctor" {
		c.manifest.PlannedMeasurements = 0
		c.manifest.PayloadBound = 0
	}
	self, err := os.Executable()
	if err != nil {
		return err
	}
	c.manifest.ControllerSHA, _, err = fileSHA(self)
	if err != nil {
		return err
	}
	return c.execute(ctx, args[0], *allowShared)
}

type controller struct {
	config                                    lab
	output, public, private, source, snapshot string
	nodes                                     []*node
	manifest                                  manifest
	heartbeatCancel                           context.CancelFunc
	heartbeatCtx                              context.Context
	heartbeatWG                               sync.WaitGroup
}

func (c *controller) execute(ctx context.Context, mode string, allowShared bool) (result error) {
	defer func() {
		if c.heartbeatCancel != nil {
			c.heartbeatCancel()
			c.heartbeatWG.Wait()
		}
		for _, n := range c.nodes {
			if err := n.close(c.manifest.RunID, c.manifest.KeepPrivate); err != nil {
				c.manifest.CleanupErrors = append(c.manifest.CleanupErrors, err.Error())
			}
			for i := range c.manifest.Nodes {
				if c.manifest.Nodes[i].Name == n.name {
					c.manifest.Nodes[i].Cleanup = n.cleanup
				}
			}
		}
		if len(c.manifest.CleanupErrors) > 0 && result == nil {
			result = fmt.Errorf("owned_node_cleanup_failed")
		}
		if c.snapshot != "" && len(c.manifest.Source.Files) > 0 {
			if err := checkSnapshot(c.snapshot, c.manifest.Source); err != nil && result == nil {
				result = err
			} else if err == nil {
				c.manifest.SnapshotUnchanged = true
			}
		}
		if !c.manifest.KeepPrivate && len(c.manifest.CleanupErrors) == 0 {
			if err := os.RemoveAll(c.private); err != nil {
				c.manifest.CleanupErrors = append(c.manifest.CleanupErrors, "controller_private_cleanup_failed")
				if result == nil {
					result = fmt.Errorf("controller_private_cleanup_failed")
				}
			} else {
				c.manifest.PrivateRemoved = true
			}
		}
		c.manifest.Status = "passed"
		if result != nil {
			c.manifest.Status = "failed"
			c.manifest.Failure = result.Error()
		}
		c.manifest.Finished = time.Now().UTC()
		if err := c.save(); err != nil && result == nil {
			result = err
		}
		if err := c.report(); err != nil && result == nil {
			result = err
		}
		fmt.Println("REPORT", filepath.Join(c.public, "report.md"))
	}()
	if err := c.save(); err != nil {
		return err
	}
	c.snapshot = filepath.Join(c.private, "source")
	fmt.Println("SNAPSHOT freezing build inputs")
	source, err := freezeSource(ctx, c.source, c.snapshot)
	if err != nil {
		return err
	}
	c.manifest.Source = source
	b, err := command(ctx, c.snapshot, nil, "go", "version")
	if err != nil {
		return err
	}
	c.manifest.Go = strings.TrimSpace(string(b))
	targets := map[string]platform{}
	for _, name := range labels(c.config) {
		p, err := detect(ctx, c.config.Nodes[name])
		if err != nil {
			return fmt.Errorf("%s: %s", name, err)
		}
		n := &node{name: name, config: c.config.Nodes[name], platform: p, private: filepath.Join(c.private, name+"-control.log")}
		c.nodes = append(c.nodes, n)
		targets[p.OS+"-"+p.Arch] = p
		fmt.Printf("PROBE %s %s/%s\n", name, p.OS, p.Arch)
	}
	builds := map[string]map[string]string{}
	keys := []string{}
	for key := range targets {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	for _, key := range keys {
		p := targets[key]
		fmt.Println("BUILD", key)
		hashes, err := buildTarget(ctx, c.snapshot, filepath.Join(c.private, "artifacts", key), p.OS, p.Arch, mode == "run")
		if err != nil {
			return err
		}
		builds[key] = hashes
	}
	c.heartbeatCtx, c.heartbeatCancel = context.WithCancel(context.Background())
	privateNodes := map[string]any{}
	machineNames := map[string]string{}
	for _, n := range c.nodes {
		key := n.platform.OS + "-" + n.platform.Arch
		n.hashes = builds[key]
		suffix := ""
		if n.platform.OS == "windows" {
			suffix = ".exe"
		}
		if err := n.bootstrap(ctx, filepath.Join(c.private, "artifacts", key, "unic-e2e-worker"+suffix), c.manifest.RunID); err != nil {
			return err
		}
		c.startHeartbeat(n)
		privateNodes[n.name] = map[string]any{"host": n.config.Host, "root": n.hello.Root, "socket": n.hello.Socket, "bootstrap": n.platform.Bootstrap}
		if err = writeJSON(filepath.Join(c.private, "nodes.json"), privateNodes); err != nil {
			return err
		}
		h := n.hello
		if h.Protocol != protocolVersion || h.OS != n.platform.OS || h.Arch != n.platform.Arch || h.WorkerSHA != n.hashes["unic-e2e-worker"+suffix] {
			return fmt.Errorf("%s: worker_identity_mismatch", n.name)
		}
		c.manifest.Nodes = append(c.manifest.Nodes, nodeEvidence{Name: n.name, Transport: n.config.Transport, OS: h.OS, Arch: h.Arch, Machine: h.Machine, Free: h.FreeBytes, Programs: n.hashes})
		if previous, ok := machineNames[h.Machine]; ok && !allowShared {
			return fmt.Errorf("duplicate_machine_identity: %s and %s", previous, n.name)
		}
		machineNames[h.Machine] = n.name
		c.manifest.MachineCount = len(machineNames)
		if h.FreeBytes < c.requiredSpace() {
			return fmt.Errorf("%s: insufficient_free_space", n.name)
		}
		if mode == "run" {
			hashes := map[string]string{}
			for _, name := range []string{"unic" + suffix, "unicd" + suffix} {
				if err = n.copy(ctx, filepath.Join(c.private, "artifacts", key, name), strings.ReplaceAll(h.Root, "\\", "/")+"/bin/"+name); err != nil {
					return err
				}
				hashes[name] = n.hashes[name]
			}
			plugins, err := c.plugins()
			if err != nil {
				return err
			}
			if err = n.action(ctx, "configure", installConfig{Name: n.name, Hashes: hashes, Plugins: plugins}); err != nil {
				return err
			}
		}
		fmt.Println("READY", n.name)
	}
	if err = c.check("node connectivity, unique-machine policy, private workspace, free space and artifact hashes"); err != nil {
		return err
	}
	if mode == "doctor" {
		return nil
	}
	if err := c.runScenarios(ctx); err != nil {
		return err
	}
	if len(c.manifest.Samples) != c.manifest.PlannedMeasurements {
		return fmt.Errorf("incomplete_measurement_matrix")
	}
	return nil
}
func (c *controller) requiredSpace() uint64 {
	max := int64(0)
	for _, p := range payloads(c.config.Suite) {
		size := p.Size
		if p.Kind == "png" {
			size = size * size * 4
		}
		if p.Kind == "html" {
			size *= 2
		}
		if size > max {
			max = size
		}
	}
	return uint64(128<<20 + max*4)
}
func (c *controller) plugins() (map[string][]byte, error) {
	out := map[string][]byte{}
	for _, name := range []string{"sensitive", "emailtag"} {
		for _, file := range []string{"spec.toml", "main.lua"} {
			b, err := os.ReadFile(filepath.Join(c.snapshot, "plugins", "examples", "lua", name, file))
			if err != nil {
				return nil, fmt.Errorf("plugin_fixture_missing")
			}
			out[name+"/"+file] = b
		}
	}
	return out, nil
}
func (c *controller) startHeartbeat(n *node) {
	c.heartbeatWG.Add(1)
	go func() {
		defer c.heartbeatWG.Done()
		ticker := time.NewTicker(15 * time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-c.heartbeatCtx.Done():
				return
			case <-ticker.C:
				probe, done := context.WithTimeout(context.Background(), 15*time.Second)
				_ = n.action(probe, "clock", nil)
				done()
			}
		}
	}()
}
