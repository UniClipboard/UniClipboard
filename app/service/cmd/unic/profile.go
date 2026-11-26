package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"
	"text/tabwriter"
	"time"

	"github.com/uniclipboard/unic/internal/climigration"
	api "github.com/uniclipboard/unic/internal/localapi"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

const profileUsage = `List and inspect local unic profiles.

Usage:
  unic profile [command] [flags]

Commands:
  list, ls [--json]    List local profiles and their daemon status (default)
  path                 Print the active profile directory path

Examples:
  unic profile list
  unic --profile work profile path
`

type profileRow struct {
	Name        string `json:"name"`
	Active      bool   `json:"active"`
	Default     bool   `json:"default"`
	Initialized string `json:"initialized"`
	Daemon      string `json:"daemon"`
	PID         int    `json:"pid,omitempty"`
	Directory   string `json:"directory"`
}

func runProfileCmd(ctx context.Context, sub string, args []string, asJSONDefault bool) error {
	if wantsHelp(args) || sub == "help" {
		fmt.Print(profileUsage)
		return nil
	}
	switch sub {
	case "", "list", "ls":
		fs := flag.NewFlagSet("profile list", flag.ContinueOnError)
		fs.SetOutput(io.Discard)
		asJSON := fs.Bool("json", asJSONDefault, "output JSON")
		pos, err := parse(fs, args)
		if err != nil || len(pos) != 0 {
			return usageError("usage: unic profile list [--json]")
		}
		list, err := paths.ListProfiles()
		if err != nil {
			return err
		}
		rows := make([]profileRow, 0, len(list))
		for _, p := range list {
			initState := "no"
			if p.Exists {
				st, rerr := climigration.Read(p.Dir)
				switch {
				case rerr != nil:
					initState = "corrupt"
				case st.Status == "complete":
					initState = "yes"
				case st.Status == "pending":
					initState = "pending"
				}
			}
			daemonState := "stopped"
			pid := 0
			if p.Exists {
				pctx, cancel := context.WithTimeout(ctx, 250*time.Millisecond)
				inst, serr := api.NewClient(p.Socket).Service(pctx)
				cancel()
				if serr == nil && inst.Directory == p.Dir {
					daemonState = fmt.Sprintf("running (%s)", inst.Mode)
					pid = inst.PID
				}
			}
			rows = append(rows, profileRow{
				Name:        p.Name,
				Active:      p.Active,
				Default:     p.Default,
				Initialized: initState,
				Daemon:      daemonState,
				PID:         pid,
				Directory:   p.Dir,
			})
		}
		if *asJSON {
			return json.NewEncoder(os.Stdout).Encode(rows)
		}
		w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
		_, _ = fmt.Fprintln(w, "PROFILE\tACTIVE\tINITIALIZED\tDAEMON\tDIRECTORY")
		for _, r := range rows {
			act := ""
			if r.Active {
				act = "*"
			}
			_, _ = fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%s\n", r.Name, act, r.Initialized, r.Daemon, r.Directory)
		}
		return w.Flush()

	case "path":
		if len(args) != 0 {
			return usageError("usage: unic profile path")
		}
		dir, err := paths.CanonicalDir()
		if err != nil {
			return err
		}
		fmt.Println(dir)
		return nil

	default:
		return usageError(strings.TrimRight(profileUsage, "\n"))
	}
}
