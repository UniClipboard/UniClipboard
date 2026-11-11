package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"strings"

	"golang.org/x/term"

	"github.com/uniclipboard/unic/internal/v1migration"
)

const migrationGuide = `UniClipboard v1 to v2 history migration

v2 cannot sync with v1 or directly use its database, settings or device identity.
Upgrade all devices to v2 and pair them again. Configure v2 settings separately.

1. Back up your v1 data. Keep the backup until you have checked the v2 history.
2. Quit v1 and stop unicd for the target profile. Migration does not stop them.
3. Check recoverable history without writing the target:
     unic migrate --dry-run --json
4. Import history and verify the copied content:
     unic migrate --json
5. Check the report and v2 history before starting unicd.

Default source: the local v1 desktop data directory.
Default target: UNIC_HOME or the normal unic data directory.
Custom directories: add --source DIR --target DIR to both commands.
Named profiles: add --source DIR --service UniClipboard-PROFILE for that profile.

Migration imports supported text, HTML, images, files and tags. It does not
import settings, device identities, trust or favorites. Missing or damaged
content is reported as partial. Exit codes: 0 ready/complete, 3 partial, 1 failed.
The source remains unchanged. Rerun the same command after an interruption;
successful records are not duplicated. A dry run is not a completed import.

Allow the system credential prompt or enter the old passphrase when requested.
For automation use --passphrase-stdin. Never put a passphrase in arguments or
environment variables. Custom or portable v1 profiles require --source DIR.
`

// runMigration imports local UniClipboard v1 history in-process, without
// starting networking or clipboard integration. It returns the exit code:
// 0 complete, 3 partial, 1 failed, 2 usage.
func runMigration(args []string, in *os.File, out, errOut io.Writer) int {
	if len(args) == 0 || len(args) == 1 && (args[0] == "--help" || args[0] == "-h" || args[0] == "help") {
		if _, err := fmt.Fprint(out, migrationGuide); err != nil {
			return 1
		}
		return 0
	}
	fs := flag.NewFlagSet("unic migrate", flag.ContinueOnError)
	fs.SetOutput(errOut)
	source := fs.String("source", "", "v1 data directory (default: local desktop profile)")
	target := fs.String("target", "", "unic data directory (default: UNIC_HOME or normal unic directory)")
	service := fs.String("service", "UniClipboard", "old credential service; named profiles use UniClipboard-PROFILE")
	dry := fs.Bool("dry-run", false, "read and authenticate content without changing the target")
	jsonMode := fs.Bool("json", false, "emit JSON progress and final report")
	stdinPassword := fs.Bool("passphrase-stdin", false, "read old passphrase as one line from stdin; never pass it as an argument")
	if err := fs.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return 0
		}
		return 2
	}
	if fs.NArg() != 0 {
		_, _ = fmt.Fprintln(errOut, "unexpected positional arguments")
		return 2
	}
	if *source == "" {
		var err error
		*source, err = v1migration.DefaultSource()
		if err != nil {
			_, _ = fmt.Fprintln(errOut, err)
			return 1
		}
	}
	if *target == "" {
		*target = os.Getenv("UNIC_HOME")
		if *target == "" {
			base, err := os.UserConfigDir()
			if err != nil {
				_, _ = fmt.Fprintln(errOut, "cannot locate unic data directory")
				return 1
			}
			*target = filepath.Join(base, "unic")
		}
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	defer cancel()
	var password []byte
	if *stdinPassword {
		line, err := bufio.NewReader(io.LimitReader(in, 4097)).ReadString('\n')
		if err != nil && err != io.EOF || len(line) > 4096 {
			_, _ = fmt.Fprintln(errOut, "cannot read old passphrase")
			return 1
		}
		password = []byte(strings.TrimSuffix(strings.TrimSuffix(line, "\n"), "\r"))
	}
	defer func() { clear(password) }()
	if *jsonMode {
		_ = json.NewEncoder(out).Encode(map[string]string{"stage": "unlock"})
	} else {
		_, _ = fmt.Fprintln(errOut, "Unlocking old history; allow the system credential prompt or enter the old passphrase if requested.")
	}
	s, err := v1migration.Open(ctx, *source, *service, password, v1migration.ReadSecret)
	if errors.Is(err, v1migration.ErrUnlock) && !*stdinPassword && term.IsTerminal(int(in.Fd())) {
		if *jsonMode {
			_ = json.NewEncoder(out).Encode(map[string]string{"stage": "needs_passphrase"})
		}
		_, _ = fmt.Fprint(errOut, "Old UniClipboard passphrase: ")
		password, err = term.ReadPassword(int(in.Fd()))
		_, _ = fmt.Fprintln(errOut)
		if err == nil {
			s, err = v1migration.Open(ctx, *source, *service, password, nil)
		}
	}
	clear(password)
	if err != nil {
		_, _ = fmt.Fprintln(errOut, err)
		return 1
	}
	defer func() { _ = s.Close() }()
	var lastStage string
	report, err := s.Run(ctx, v1migration.RunOptions{Target: *target, DryRun: *dry, Progress: func(p v1migration.Progress) {
		if p.Stage != lastStage || p.Processed == 1 || p.Processed%25 == 0 || p.Processed == p.Total {
			if *jsonMode {
				_ = json.NewEncoder(out).Encode(p)
			} else {
				_, _ = fmt.Fprintf(errOut, "%s %d/%d\n", p.Stage, p.Processed, p.Total)
			}
			lastStage = p.Stage
		}
	}})
	if *jsonMode {
		_ = json.NewEncoder(out).Encode(report)
	} else {
		_, _ = fmt.Fprintf(out, "%s: total=%d recoverable=%d imported=%d skipped=%d failed=%d missing_parts=%d verified=%d verification_failed=%d tags=%d\n",
			report.Status, report.Total, report.Recoverable, report.Imported, report.Skipped, report.Failed, report.MissingParts, report.Verified, report.VerificationFailed, report.Tags)
	}
	if err != nil {
		_, _ = fmt.Fprintln(errOut, err)
		return 1
	}
	if report.Status == "partial" {
		return 3
	}
	return 0
}
