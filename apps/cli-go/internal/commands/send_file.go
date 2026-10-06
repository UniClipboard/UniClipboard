package commands

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"time"
	"unicode/utf8"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// deliveryPollInterval is the delivery-view poll interval after a file send.
const deliveryPollInterval = 250 * time.Millisecond

// dispatchFileOutcome mirrors `DispatchFileOutcomeResponse`.
type dispatchFileOutcome struct {
	EntryID        string             `json:"entryId"`
	SnapshotHash   string             `json:"snapshotHash"`
	AtMs           int64              `json:"atMs"`
	TotalAccepted  int                `json:"totalAccepted"`
	TotalDuplicate int                `json:"totalDuplicate"`
	TotalOffline   int                `json:"totalOffline"`
	TotalErrored   int                `json:"totalErrored"`
	TotalPending   int                `json:"totalPending"`
	PerTarget      []perTargetOutcome `json:"perTarget"`
}

// deliveryTarget mirrors `EntryDeliveryTargetDto`; the tagged status is kept
// as the daemon's bytes.
type deliveryTarget struct {
	TargetDeviceID   string          `json:"targetDeviceId"`
	TargetDeviceName *string         `json:"targetDeviceName"`
	Status           json.RawMessage `json:"status"`
	ReasonDetail     *string         `json:"reasonDetail"`
	UpdatedAtMs      *int64          `json:"updatedAtMs"`
}

func (t *deliveryTarget) statusTag() string {
	var status struct {
		Tag string `json:"tag"`
	}
	json.Unmarshal(t.Status, &status)
	return status.Tag
}

// deliveryView holds the `EntryDeliveryViewDto` fields the CLI reads.
type deliveryView struct {
	Deliveries []deliveryTarget `json:"deliveries"`
}

// sendFileOutcome mirrors the CLI's `SendFileOutcomeDto`.
type sendFileOutcome struct {
	EntryID        string             `json:"entryId"`
	SnapshotHash   string             `json:"snapshotHash"`
	Filename       string             `json:"filename"`
	SizeBytes      uint64             `json:"sizeBytes"`
	TotalAccepted  int                `json:"totalAccepted"`
	TotalDuplicate int                `json:"totalDuplicate"`
	TotalOffline   int                `json:"totalOffline"`
	TotalErrored   int                `json:"totalErrored"`
	PerTarget      []perTargetOutcome `json:"perTarget"`
	Deliveries     []deliveryTarget   `json:"deliveries"`
}

type fileSendResult struct {
	exitCode int
	outcome  *sendFileOutcome
}

type dispatchFileRequest struct {
	SourcePath string   `json:"sourcePath"`
	Peers      []string `json:"peers"`
}

func runSendFilesViaDaemon(client *daemonclient.Client, paths []string, peers []string, asJSON bool, interrupts <-chan os.Signal) int {
	code := exitcode.Success
	outcomes := []*sendFileOutcome{}
	for _, path := range paths {
		result := runSendFileViaDaemon(client, path, peers, asJSON, false, interrupts)
		if result.exitCode != exitcode.Success {
			code = result.exitCode
		}
		if result.outcome != nil {
			outcomes = append(outcomes, result.outcome)
		}
	}
	if asJSON {
		rendered, err := output.Pretty(outcomes)
		if err != nil {
			ui.Error(fmt.Sprintf("Failed to serialize outcomes: %v", err))
			return exitcode.Error
		}
		fmt.Println(rendered)
	}
	return code
}

func runSendFileViaDaemon(client *daemonclient.Client, path string, peers []string, asJSON, emitJSON bool, interrupts <-chan os.Signal) fileSendResult {
	failed := fileSendResult{exitCode: exitcode.Error}
	if !utf8.ValidString(path) {
		ui.Error("File path is not valid Unicode.")
		return failed
	}
	info, err := os.Stat(path)
	if err != nil {
		ui.Error("Failed to inspect file: " + rustIOError(err))
		return failed
	}
	filename := filepath.Base(path)
	if filename == "" || filename == "." || filename == string(filepath.Separator) {
		filename = "file"
	}

	// Subscribe before dispatch so no early progress is missed. Progress is
	// optional: a failed subscription never blocks the send.
	activityCtx, stopActivity := context.WithCancel(context.Background())
	defer stopActivity()
	var activity <-chan daemonclient.Event
	if ws, err := client.DialWS(activityCtx); err == nil {
		defer ws.Close()
		if ws.Subscribe(activityCtx, "clipboard", "file-transfer") == nil {
			activity = ws.Events(activityCtx)
		}
	}

	spinner := ui.NewSpinner("Dispatching file via daemon...")
	var outcome dispatchFileOutcome
	err = client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodPost,
		Path:   "/clipboard/dispatch-file",
		JSON:   dispatchFileRequest{SourcePath: path, Peers: peers},
	}, &outcome)
	if err != nil {
		spinner.FinishError("File send failed: " + err.Error())
		return failed
	}
	spinner.FinishSuccess(fmt.Sprintf("%d accepted, %d duplicate, %d offline, %d error(s)",
		outcome.TotalAccepted, outcome.TotalDuplicate, outcome.TotalOffline, outcome.TotalErrored))

	accepted := map[string]bool{}
	related := map[string]bool{}
	for _, target := range outcome.PerTarget {
		if target.Outcome == "accepted" {
			accepted[target.DeviceID] = true
		}
		related[target.DeviceID] = true
	}
	var view *deliveryView
	if len(accepted) > 0 {
		interactive := !asJSON && ui.StderrIsTerminal()
		progress := newFileProgress(interactive, outcome.EntryID, len(accepted), filename)
		waited, cancelled, err := waitForFileDelivery(client, outcome.EntryID, accepted, activity, progress, interrupts)
		progress.finish()
		switch {
		case cancelled:
			ui.Warn("Cancelled while waiting; the daemon may continue active transfers.")
			return failed
		case err != nil:
			ui.Error("Failed to query file delivery: " + err.Error())
			return failed
		}
		view = waited
	}

	if outcome.PerTarget == nil {
		outcome.PerTarget = []perTargetOutcome{}
	}
	result := &sendFileOutcome{
		EntryID:        outcome.EntryID,
		SnapshotHash:   outcome.SnapshotHash,
		Filename:       filename,
		SizeBytes:      uint64(info.Size()),
		TotalAccepted:  outcome.TotalAccepted,
		TotalDuplicate: outcome.TotalDuplicate,
		TotalOffline:   outcome.TotalOffline,
		TotalErrored:   outcome.TotalErrored,
		PerTarget:      outcome.PerTarget,
		Deliveries:     []deliveryTarget{},
	}
	if view != nil {
		for _, target := range view.Deliveries {
			if related[target.TargetDeviceID] {
				result.Deliveries = append(result.Deliveries, target)
			}
		}
	}
	if asJSON && emitJSON {
		rendered, err := output.Pretty(result)
		if err != nil {
			ui.Error(fmt.Sprintf("Failed to serialize outcome: %v", err))
			return failed
		}
		fmt.Println(rendered)
	} else if !asJSON {
		ui.Bar()
		ui.Info("file", result.Filename)
		ui.Info("size", humanSize(result.SizeBytes))
		ui.Info("hash", shortHash(result.SnapshotHash))
		for i := range result.Deliveries {
			target := &result.Deliveries[i]
			ui.Info("·", fmt.Sprintf("%s → %s", target.TargetDeviceID, deliveryStatusLabel(target.statusTag())))
		}
		if len(result.Deliveries) == 0 {
			for _, target := range result.PerTarget {
				ui.Info("·", fmt.Sprintf("%s → %s", target.DeviceID, target.Outcome))
			}
		}
		ui.Bar()
		ui.End("File send finished")
	}
	code := exitcode.Success
	if result.TotalAccepted == 0 && result.TotalDuplicate == 0 {
		code = exitcode.Error
	} else {
		for i := range result.Deliveries {
			if result.Deliveries[i].statusTag() == "failed" {
				code = exitcode.Error
				break
			}
		}
	}
	return fileSendResult{exitCode: code, outcome: result}
}

// waitForFileDelivery polls the delivery view until every accepted target
// is terminal. Each poll listens for Ctrl-C afresh, like the Rust loop: an
// interrupt that arrives during a request is not observed.
func waitForFileDelivery(client *daemonclient.Client, entryID string, accepted map[string]bool, activity <-chan daemonclient.Event, progress *fileProgress, interrupts <-chan os.Signal) (*deliveryView, bool, error) {
	segment, err := daemonclient.PathSegment(entryID)
	if err != nil {
		return nil, false, err
	}
	path := "/clipboard/entries/" + segment + "/delivery"
	for {
	drain:
		for activity != nil {
			select {
			case event, ok := <-activity:
				if !ok {
					activity = nil
					break drain
				}
				progress.observe(event)
			default:
				break drain
			}
		}
		var view deliveryView
		if err := client.Get(context.Background(), path, &view); err != nil {
			return nil, false, err
		}
		if allTargetsTerminal(&view, accepted) {
			return &view, false, nil
		}
		progress.refresh()
		select {
		case <-interrupts:
		default:
		}
		select {
		case <-interrupts:
			return nil, true, nil
		case <-time.After(deliveryPollInterval):
		}
	}
}

func allTargetsTerminal(view *deliveryView, targets map[string]bool) bool {
	for id := range targets {
		terminal := false
		for i := range view.Deliveries {
			if view.Deliveries[i].TargetDeviceID == id {
				terminal = view.Deliveries[i].statusTag() != "pending"
				break
			}
		}
		if !terminal {
			return false
		}
	}
	return true
}

func deliveryStatusLabel(tag string) string {
	switch tag {
	case "pending":
		return "pending"
	case "delivered":
		return "delivered"
	case "duplicate":
		return "duplicate"
	case "unreachable":
		return "offline"
	case "superseded":
		return "superseded"
	default:
		return "failed"
	}
}

// humanSize mirrors the send command's `human_size` (binary units up to GiB).
func humanSize(bytes uint64) string {
	const (
		kib = 1024
		mib = 1024 * kib
		gib = 1024 * mib
	)
	switch {
	case bytes >= gib:
		return fmt.Sprintf("%.2f GiB", float64(bytes)/gib)
	case bytes >= mib:
		return fmt.Sprintf("%.2f MiB", float64(bytes)/mib)
	case bytes >= kib:
		return fmt.Sprintf("%.2f KiB", float64(bytes)/kib)
	default:
		return fmt.Sprintf("%d B", bytes)
	}
}

// fileProgress is the interactive byte progress for one outbound file.
//
// Bytes come from the receiving peers' fetch-progress reports relayed by the
// Engine (`direction: sending`, keyed by the entry id); they measure what the
// peers have received. With several targets the bar sums bytes across targets
// and stays a spinner until every accepted target has reported a known total.
// Terminal outcomes always come from the delivery view, never from this display.
type fileProgress struct {
	interactive     bool
	entryID         string
	expectedTargets int
	label           string
	reports         map[string]*progressReport
	bar             *ui.Spinner
	hasLength       bool
}

type progressReport struct {
	bytes uint64
	total *uint64
}

type fileTransferProgress struct {
	EntryID          *string `json:"entryId"`
	PeerID           string  `json:"peerId"`
	Direction        string  `json:"direction"`
	BytesTransferred uint64  `json:"bytesTransferred"`
	TotalBytes       *uint64 `json:"totalBytes"`
}

func newFileProgress(interactive bool, entryID string, expectedTargets int, filename string) *fileProgress {
	return &fileProgress{
		interactive:     interactive,
		entryID:         entryID,
		expectedTargets: expectedTargets,
		label:           "Sending " + filename,
		reports:         map[string]*progressReport{},
	}
}

func (p *fileProgress) observe(event daemonclient.Event) {
	if event.Type != "file-transfer.progress" {
		return
	}
	var payload fileTransferProgress
	if json.Unmarshal(event.Payload, &payload) != nil {
		return
	}
	if payload.Direction != "sending" || payload.EntryID == nil || *payload.EntryID != p.entryID {
		return
	}
	report, ok := p.reports[payload.PeerID]
	if !ok {
		report = &progressReport{}
		p.reports[payload.PeerID] = report
	}
	// Keep the display monotonic; reports are throttled and may reorder.
	report.bytes = max(report.bytes, payload.BytesTransferred)
	if payload.TotalBytes != nil {
		report.total = payload.TotalBytes
	}
}

// aggregate returns (received, total) once every accepted target has a known
// non-zero total.
func (p *fileProgress) aggregate() (uint64, uint64, bool) {
	if len(p.reports) < p.expectedTargets {
		return 0, 0, false
	}
	var received, total uint64
	for _, report := range p.reports {
		if report.total == nil {
			return 0, 0, false
		}
		received += min(report.bytes, *report.total)
		total += *report.total
	}
	return received, total, total > 0
}

func (p *fileProgress) refresh() {
	if !p.interactive {
		return
	}
	received, total, known := p.aggregate()
	switch {
	case known:
		if !p.hasLength {
			if p.bar != nil {
				p.bar.Clear()
			}
			p.bar = ui.NewByteProgress(total, p.label)
			p.hasLength = true
		}
		p.bar.SetLength(total)
		p.bar.SetPosition(received)
	case p.bar == nil:
		p.bar = ui.NewSpinner(p.label + ": waiting for the receiving device...")
	}
}

func (p *fileProgress) finish() {
	if p.bar != nil {
		p.bar.Clear()
		p.bar = nil
	}
}
