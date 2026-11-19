package main

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

type clockEstimate struct {
	OffsetMS      float64 `json:"offset_ms"`
	UncertaintyMS float64 `json:"uncertainty_ms"`
	ControllerMS  float64 `json:"controller_ms"`
}
type calibration struct {
	Scenario string        `json:"scenario"`
	Cell     string        `json:"cell"`
	Node     string        `json:"node"`
	Position string        `json:"position"`
	Clock    clockEstimate `json:"clock"`
}
type sample struct {
	ReceivedParts    []expectedPart `json:"received_parts"`
	ReceiptID        string         `json:"receipt_id"`
	HistoryID        string         `json:"history_id"`
	Scenario         string         `json:"scenario"`
	Case             string         `json:"case"`
	Sender           string         `json:"sender"`
	Receiver         string         `json:"receiver"`
	Kind             string         `json:"kind"`
	NominalSize      int64          `json:"nominal_size"`
	Bytes            int64          `json:"bytes"`
	Round            int            `json:"round"`
	Entry            string         `json:"entry"`
	Parts            []expectedPart `json:"parts"`
	HashVerified     bool           `json:"hash_verified"`
	Count            int            `json:"delivery_count"`
	SubmitNS         int64          `json:"sender_started_ns"`
	ReceiveMS        int64          `json:"receiver_received_ms"`
	SubmitMS         float64        `json:"submit_ms"`
	VerifiedMS       float64        `json:"verified_ms"`
	EstimateMS       float64        `json:"receive_estimate_ms"`
	UncertaintyMS    float64        `json:"uncertainty_ms"`
	LowerMS          float64        `json:"receive_lower_ms"`
	UpperMS          float64        `json:"receive_upper_ms"`
	SenderOffsetMS   float64        `json:"sender_offset_ms"`
	ReceiverOffsetMS float64        `json:"receiver_offset_ms"`
	FanoutLastMS     float64        `json:"fanout_last_estimate_ms"`
	Path             string         `json:"network_path"`
}
type nodeEvidence struct {
	Name      string            `json:"name"`
	Transport string            `json:"transport"`
	OS        string            `json:"os"`
	Arch      string            `json:"arch"`
	Machine   string            `json:"run_scoped_machine_identity"`
	Free      uint64            `json:"free_bytes_at_start"`
	Programs  map[string]string `json:"program_sha256"`
	Cleanup   *logsResult       `json:"cleanup,omitempty"`
}
type targetOutcome struct {
	Scenario     string         `json:"scenario"`
	Case         string         `json:"case"`
	Sender       string         `json:"sender"`
	Receiver     string         `json:"receiver"`
	Kind         string         `json:"kind"`
	Bytes        int64          `json:"bytes"`
	Expected     []expectedPart `json:"expected_parts"`
	Actual       []expectedPart `json:"received_parts,omitempty"`
	HashVerified bool           `json:"hash_verified"`
	VerifiedMS   float64        `json:"verified_ms"`
	Error        string         `json:"error,omitempty"`
}
type manifest struct {
	KeepPrivate         bool            `json:"keep_private"`
	PrivateRemoved      bool            `json:"controller_private_removed"`
	Outcomes            []targetOutcome `json:"target_outcomes"`
	PlannedMeasurements int             `json:"planned_measurements"`
	PayloadBound        int64           `json:"planned_payload_bytes_upper_bound"`
	Version             int             `json:"version"`
	RunID               string          `json:"run_id"`
	Mode                string          `json:"mode"`
	Status              string          `json:"status"`
	Started             time.Time       `json:"started_at"`
	Finished            time.Time       `json:"finished_at"`
	Source              sourceIdentity  `json:"source"`
	SnapshotUnchanged   bool            `json:"snapshot_unchanged"`
	Go                  string          `json:"go"`
	ControllerSHA       string          `json:"controller_sha256"`
	Nodes               []nodeEvidence  `json:"nodes"`
	MachineCount        int             `json:"distinct_machine_identities"`
	SharedHostControl   bool            `json:"shared_host_control"`
	Suite               suiteConfig     `json:"suite"`
	Checks              []string        `json:"checks"`
	Calibrations        []calibration   `json:"calibrations"`
	Samples             []sample        `json:"samples"`
	Failure             string          `json:"failure,omitempty"`
	CleanupErrors       []string        `json:"cleanup_errors,omitempty"`
	Limits              []string        `json:"limits"`
}

func writeJSON(path string, v any) error {
	b, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return err
	}
	b = append(b, '\n')
	temp := path + ".tmp"
	if err = os.WriteFile(temp, b, 0600); err != nil {
		return err
	}
	return os.Rename(temp, path)
}
func (c *controller) save() error {
	return writeJSON(filepath.Join(c.public, "manifest.json"), c.manifest)
}
func (c *controller) check(name string) error {
	c.manifest.Checks = append(c.manifest.Checks, name)
	fmt.Println("PASS", name)
	return c.save()
}
func median(v []float64) float64 {
	sort.Float64s(v)
	if len(v)%2 == 1 {
		return v[len(v)/2]
	}
	return (v[len(v)/2-1] + v[len(v)/2]) / 2
}
func (c *controller) report() error {
	var raw strings.Builder
	for _, s := range c.manifest.Samples {
		b, err := json.Marshal(s)
		if err != nil {
			return err
		}
		raw.Write(b)
		raw.WriteByte('\n')
	}
	if err := os.WriteFile(filepath.Join(c.public, "results.jsonl"), []byte(raw.String()), 0600); err != nil {
		return err
	}
	var outcomes strings.Builder
	for _, out := range c.manifest.Outcomes {
		b, err := json.Marshal(out)
		if err != nil {
			return err
		}
		outcomes.Write(b)
		outcomes.WriteByte('\n')
	}
	if err := os.WriteFile(filepath.Join(c.public, "outcomes.jsonl"), []byte(outcomes.String()), 0600); err != nil {
		return err
	}

	var text strings.Builder
	fmt.Fprintf(&text, "# Real-machine CLI harness\n\nStatus: **%s**. Run: `%s`.\n\n", c.manifest.Status, c.manifest.RunID)
	fmt.Fprintf(&text, "Keep private artifacts: %t. Controller private directory removed: %t. Cleanup errors: %d.\n\n", c.manifest.KeepPrivate, c.manifest.PrivateRemoved, len(c.manifest.CleanupErrors))
	fmt.Fprintf(&text, "Nodes: %d. Distinct operating-system machine identities: %d. Shared-host control: %t.\n\n", len(c.manifest.Nodes), c.manifest.MachineCount, c.manifest.SharedHostControl)
	fmt.Fprintf(&text, "Source HEAD: `%s`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.\n\n", c.manifest.Source.Head)
	text.WriteString("## Nodes\n\n| Node | Execution | Target | Initial free bytes |\n|---|---|---|---:|\n")
	for _, n := range c.manifest.Nodes {
		fmt.Fprintf(&text, "| %s | %s | %s/%s | %d |\n", n.Name, n.Transport, n.OS, n.Arch, n.Free)
	}
	fmt.Fprintf(&text, "\nVerified measurement rows: %d / %d. Planned payload upper bound: %d bytes, excluding setup and transport overhead.\n\n", len(c.manifest.Samples), c.manifest.PlannedMeasurements, c.manifest.PayloadBound)
	text.WriteString("## Checks\n\n")
	for _, v := range c.manifest.Checks {
		fmt.Fprintf(&text, "- PASS: %s\n", v)
	}
	if c.manifest.Failure != "" {
		fmt.Fprintf(&text, "\nFailure: %s\n", c.manifest.Failure)
	}
	for _, out := range c.manifest.Outcomes {
		if out.Error != "" {
			fmt.Fprintf(&text, "\nTarget failure: %s → %s, %s, %d bytes: %s\n", out.Sender, out.Receiver, out.Kind, out.Bytes, out.Error)
		}
	}
	for _, v := range c.manifest.CleanupErrors {
		fmt.Fprintf(&text, "\nCleanup failure: %s\n", v)
	}
	text.WriteString("\n## Receive latency\n\nReceive estimates include submitting-client startup, IPC, commit, transport and receiver storage. HTML uses the production local API injector. They exclude controller-side SSH, export and hash verification round trips. `verified_ms` separately includes those control costs. Native clock exchanges bracket each payload cell. Per-sample bounds include both node clock uncertainties, timestamp quantization and observed calibration drift. Bounds are clock bounds, not statistical confidence intervals. A bound reaching zero cannot establish a precise one-way latency. No network path is inferred from the SSH route.\n\n")
	type group struct{ samples []sample }
	groups := map[string]*group{}
	keys := []string{}
	for _, s := range c.manifest.Samples {
		key := fmt.Sprintf("%s | %s → %s | %s | %d", s.Scenario, s.Sender, s.Receiver, s.Kind, s.NominalSize)
		if groups[key] == nil {
			groups[key] = &group{}
			keys = append(keys, key)
		}
		groups[key].samples = append(groups[key].samples, s)
	}
	sort.Strings(keys)
	text.WriteString("| Scenario | Direction | Type | Nominal bytes / PNG edge px | n | Actual bytes | Receive median ms | Receive min–max ms | Max clock bound ±ms | Verified median ms |\n|---|---|---|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, key := range keys {
		ss := groups[key].samples
		v, verified := []float64{}, []float64{}
		uncertainty := 0.0
		for _, s := range ss {
			v = append(v, s.EstimateMS)
			verified = append(verified, s.VerifiedMS)
			uncertainty = math.Max(uncertainty, s.UncertaintyMS)
		}
		sort.Float64s(v)
		fmt.Fprintf(&text, "| %s | %d | %d | %.2f | %.2f–%.2f | %.2f | %.2f |\n", key, len(ss), ss[0].Bytes, median(v), v[0], v[len(v)-1], uncertainty, median(verified))
	}
	text.WriteString("\n## Limits\n\n")
	for _, v := range c.manifest.Limits {
		fmt.Fprintf(&text, "- %s\n", v)
	}
	if err := os.WriteFile(filepath.Join(c.public, "report.md"), []byte(text.String()), 0600); err != nil {
		return err
	}
	files := []string{"manifest.json", "report.md", "results.jsonl", "outcomes.jsonl"}
	var sums strings.Builder
	for _, name := range files {
		sha, _, err := fileSHA(filepath.Join(c.public, name))
		if err != nil {
			return err
		}
		fmt.Fprintf(&sums, "%s  %s\n", sha, name)
	}
	return os.WriteFile(filepath.Join(c.public, "SHA256SUMS"), []byte(sums.String()), 0600)
}
