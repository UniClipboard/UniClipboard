package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"runtime"
	"time"

	"github.com/uniclipboard/unic/internal/climigration"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/servicelaunch"
)

const doctorUsage = `Run read-only diagnostics on the active profile and daemon.

Usage:
  unic doctor [flags]

Flags:
  --json       Output checks as JSON
  -h, --help   Show this help

Examples:
  unic doctor
  unic --profile work doctor --json
`

type doctorCheck struct {
	Name   string `json:"name"`
	Status string `json:"status"` // "ok", "warn", "fail"
	Detail string `json:"detail"`
	Hint   string `json:"hint,omitempty"`
}

type doctorReport struct {
	Profile   string        `json:"profile"`
	Directory string        `json:"directory"`
	OK        bool          `json:"ok"`
	Checks    []doctorCheck `json:"checks"`
}

func runDoctor(ctx context.Context, args []string, asJSONDefault bool) error {
	if wantsHelp(args) {
		fmt.Print(doctorUsage)
		return nil
	}
	fs := flag.NewFlagSet("doctor", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	asJSON := fs.Bool("json", asJSONDefault, "output JSON")
	pos, err := parse(fs, args)
	if err != nil || len(pos) != 0 {
		return usageError("usage: unic doctor [--json]")
	}

	profileName, _ := paths.ActiveProfile()
	dir, err := paths.CanonicalDir()
	if err != nil {
		return err
	}
	socket, err := paths.ResolveSocket()
	if err != nil {
		return err
	}
	cfgPath, err := paths.ResolveConfig()
	if err != nil {
		return err
	}

	var checks []doctorCheck
	add := func(name, status, detail, hint string) {
		checks = append(checks, doctorCheck{Name: name, Status: status, Detail: detail, Hint: hint})
	}

	// 1. Profile directory
	if st, serr := os.Stat(dir); errors.Is(serr, os.ErrNotExist) {
		add("profile", "warn", "profile directory does not exist yet ("+dir+")", "run `unic init`")
	} else if serr != nil {
		add("profile", "fail", "cannot inspect profile directory: "+serr.Error(), "")
	} else if !st.IsDir() {
		add("profile", "fail", "profile path is not a directory: "+dir, "")
	} else if runtime.GOOS != "windows" && st.Mode().Perm()&0o077 != 0 {
		add("profile", "warn", fmt.Sprintf("profile directory %s has permissive mode %04o", dir, st.Mode().Perm()), fmt.Sprintf("chmod 700 %q", dir))
	} else {
		add("profile", "ok", dir, "")
	}

	// 2. Initialization state
	state, rerr := climigration.Read(dir)
	switch {
	case rerr != nil:
		add("initialization", "fail", rerr.Error(), "run `unic init` or `unic migrate`")
	case state.Status == "complete":
		add("initialization", "ok", "complete ("+state.Outcome+")", "")
	case state.Status == "pending":
		add("initialization", "fail", "v1 migration is incomplete", "finish with `unic migrate` (or `unic init --fresh --discard-pending` to start fresh)")
	default:
		add("initialization", "warn", "not initialized", "run `unic init` (or `unic migrate` for v1 history)")
	}

	// 3. Config file
	if raw, cerr := os.ReadFile(cfgPath); errors.Is(cerr, os.ErrNotExist) {
		add("config", "ok", "using built-in defaults (no config.toml yet)", "")
	} else if cerr != nil {
		add("config", "fail", "cannot read config.toml: "+cerr.Error(), "")
	} else if _, perr := config.Parse(raw); perr != nil {
		add("config", "fail", "invalid config.toml: "+perr.Error(), "fix "+cfgPath)
	} else {
		add("config", "ok", cfgPath, "")
	}

	// 4. Companion unicd executable
	if exe, berr := servicelaunch.Sibling(); berr != nil {
		add("binary", "fail", "cannot resolve companion unicd: "+berr.Error(), "")
	} else if cerr := servicelaunch.CheckExecutable(exe); cerr != nil {
		add("binary", "fail", cerr.Error(), "reinstall unic and unicd in the same directory")
	} else {
		add("binary", "ok", exe, "")
	}

	// 5. Daemon reachability (read-only)
	rawClient := api.NewClient(socket)
	pctx, pcancel := context.WithTimeout(ctx, probeTimeout)
	inst, serr := rawClient.Service(pctx)
	pcancel()

	daemonUp := false
	switch {
	case serr == nil && inst.Directory == dir:
		daemonUp = true
		uptime := formatDurationShort(time.Since(time.UnixMilli(inst.StartedAtMs)))
		add("daemon", "ok", fmt.Sprintf("running (%s, pid %d, up %s)", inst.Mode, inst.PID, uptime), "")
	case serr == nil:
		add("daemon", "fail", "socket belongs to a different profile ("+inst.Directory+")", "check UNIC_SOCK or --profile")
	case errors.Is(serr, api.ErrNoDaemon):
		add("daemon", "warn", "stopped", "run `unic daemon start` for continuous background sync")
	default:
		add("daemon", "fail", "cannot inspect daemon socket: "+serr.Error(), "")
	}

	// 6 & 7. Live capabilities & peers when daemon is running
	if daemonUp {
		cctx, ccancel := context.WithTimeout(ctx, probeTimeout)
		if caps, cerr := rawClient.Capabilities(cctx); cerr == nil {
			v := capabilitiesView(caps)
			add("clipboard", "ok", v.Summary, "")
		} else {
			add("clipboard", "warn", "could not query capabilities: "+cerr.Error(), "")
		}
		ccancel()

		sctx, scancel := context.WithTimeout(ctx, probeTimeout)
		if st, perr := rawClient.Status(sctx); perr == nil {
			online := 0
			for _, p := range st.Peers {
				if p.Connected {
					online++
				}
			}
			detail := fmt.Sprintf("device %s (%s); %d paired (%d online), %d pending", st.Name, st.ID, len(st.Peers), online, len(st.Pending))
			hint := ""
			if len(st.Peers) == 0 {
				hint = "run `unic device add` to pair another device"
			} else if len(st.Pending) > 0 {
				hint = "run `unic device list` to review pending approvals"
			}
			add("peers", "ok", detail, hint)
		} else {
			add("peers", "warn", "could not query peer status: "+perr.Error(), "")
		}
		scancel()
	}

	hasFail := false
	for _, c := range checks {
		if c.Status == "fail" {
			hasFail = true
			break
		}
	}
	report := doctorReport{
		Profile:   profileName,
		Directory: dir,
		OK:        !hasFail,
		Checks:    checks,
	}
	if *asJSON {
		if err := json.NewEncoder(os.Stdout).Encode(report); err != nil {
			return err
		}
		if hasFail {
			return &silentExitError{code: 1}
		}
		return nil
	}

	fmt.Printf("unic doctor  profile=%s\n\n", profileName)
	for _, c := range checks {
		badge := doctorBadge(c.Status)
		fmt.Printf("  %s  %-15s %s\n", badge, c.Name, c.Detail)
		if c.Hint != "" {
			fmt.Printf("       %-15s %s\n", "", style(outTTY, sDim, "-> "+c.Hint))
		}
	}
	if hasFail {
		return &silentExitError{code: 1}
	}
	return nil
}

func doctorBadge(status string) string {
	if !outTTY {
		switch status {
		case "ok":
			return "[OK]  "
		case "warn":
			return "[WARN]"
		default:
			return "[FAIL]"
		}
	}
	switch status {
	case "ok":
		return style(true, sOK, "✓")
	case "warn":
		return style(true, sCmd, "!")
	default:
		return style(true, sErr, "✗")
	}
}
