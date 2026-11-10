// Command unic-migrate imports local UniClipboard v1 history without starting
// either application's networking or clipboard integration.
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

	"github.com/uniclipboard/unic/internal/releaseinfo"
	"github.com/uniclipboard/unic/internal/v1migration"
)

func main() {
	if releaseinfo.Print(os.Stdout, "unic-migrate", "cli", os.Args[1:]) {
		return
	}
	os.Exit(run(os.Args[1:], os.Stdin, os.Stdout, os.Stderr))
}
func run(args []string, in *os.File, out, errOut io.Writer) int {
	fs := flag.NewFlagSet("unic-migrate", flag.ContinueOnError)
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
