package commands

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// recvOutcome mirrors the Rust `RecvOutcomeDto` JSON shape.
type recvOutcome struct {
	FromDevice   string `json:"from_device"`
	Path         string `json:"path"`
	BytesWritten uint64 `json:"bytes_written"`
	EntryID      string `json:"entry_id"`
	Outcome      string `json:"outcome"`
}

// runRecv is the deprecated single-shot inbound file receiver: it waits for
// the first remote entry with a materialized file and writes it to disk.
func runRecv(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	ui.Warn("uniclip recv is deprecated; use uniclip get --wait")
	if !asJSON {
		ui.Header("Receive file")
	}

	var dir string
	if ctx.Has("out") {
		dir = ctx.String("out")
	} else {
		cwd, err := os.Getwd()
		if err != nil {
			ui.Error("Failed to resolve current directory: " + rustIOError(err))
			return exitcode.Error
		}
		dir = cwd
	}
	outDir, err := ensureOutputDir(dir)
	if err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}

	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	wait, code := connectInboundWait(client, lease, waitEntries)
	if wait == nil {
		return code
	}
	defer wait.close()

	if !asJSON {
		ui.Info("out", outDir)
		ui.Info("status", "Waiting for incoming file — press Ctrl-C to stop")
		ui.Bar()
	}

	for {
		entry, ok, code := wait.next()
		if !ok {
			if code == exitcode.Success && !asJSON {
				ui.End("Stopped")
			}
			return code
		}
		filename, bytes, found, err := exportEntryFile(wait.client, entry.EntryID)
		if err != nil {
			ui.Error("Failed to export file: " + err.Error())
			return exitcode.Error
		}
		if !found {
			if !asJSON {
				ui.Info("·", fmt.Sprintf("entry %s carried no file — waiting for next", recvShortHash(entry.EntryID)))
			}
			continue
		}
		return finishRecvExport(outDir, entry, filename, bytes, asJSON)
	}
}

func finishRecvExport(outDir string, entry inboundEntry, filename string, bytes []byte, asJSON bool) int {
	targetPath := filepath.Join(outDir, sanitizeRecvFilename(filename))
	if err := os.WriteFile(targetPath, bytes, 0o666); err != nil {
		ui.Error("Failed to write file: " + rustIOError(err))
		return exitcode.Error
	}
	rendered := targetPath
	if asJSON {
		var err error
		rendered, err = output.Pretty(recvOutcome{
			FromDevice:   entry.FromDevice,
			Path:         targetPath,
			BytesWritten: uint64(len(bytes)),
			EntryID:      entry.EntryID,
			Outcome:      "received",
		})
		if err != nil {
			ui.Error(fmt.Sprintf("Failed to serialize receive result: %v", err))
			return exitcode.Error
		}
	}
	// Waiting and progress go to stderr; stdout carries only the result.
	fmt.Fprintln(os.Stdout, rendered)
	return exitcode.Success
}

// sanitizeRecvFilename strips path separators and NUL from a remote-supplied
// filename.
func sanitizeRecvFilename(name string) string {
	stripped := strings.Map(func(r rune) rune {
		if r == '/' || r == '\\' || r == 0 {
			return -1
		}
		return r
	}, name)
	if stripped == "" || stripped == "." || stripped == ".." {
		return "uniclip-recv.bin"
	}
	return stripped
}

// recvShortHash is the first eight bytes of an id.
func recvShortHash(s string) string {
	if len(s) > 8 {
		return s[:8]
	}
	return s
}
