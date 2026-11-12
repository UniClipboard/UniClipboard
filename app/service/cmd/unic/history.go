package main

import (
	"bufio"
	"context"
	"crypto/rand"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"github.com/uniclipboard/unic/engine"
	"io"
	"os"
	"strconv"
	"strings"
	"text/tabwriter"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

const historyUsage = `usage: unic history <command>   (alias: unic h)

  list [flags]               list entries, newest first (no contents)
      --type text,image,files   --tag t   --origin local_system,remote_device,...
      -n, --limit N (default 20)   --json
  show [ref] [flags]         print an entry's content (default: the newest)
      --part N               which part (default: the first text part)
      --json                 print the entry's details instead
  send <ref> [--to DEVICE|all] [--json]  send current source or retained snapshot
  use <ref>                 restore locally; sync follows sync.on_restore
  delete <ref>... [-y]       delete entries; asks first when run in a terminal
  tag <ref> [--add a,b] [--remove c] [--set a,b]
  expire <ref> <duration|never>   e.g. 24h, 7d, never
  export <ref> --output DIR  write user-owned copies without overwriting files
  action <ref> [key] [--apply] [--request-id KEY]         run a plugin action on a text entry (no key: list them)

<ref> is an entry id or @N, the Nth newest (@1 is the latest).
`

// historyIO is what the command talks to; tests replace it.
type historyIO struct {
	in       io.Reader
	out, err io.Writer
	tty      bool // stdin is a terminal: ask before deleting
}

func runHistory(ctx context.Context, c *api.Client, args []string) error {
	stat, _ := os.Stdin.Stat()
	tty := stat != nil && stat.Mode()&os.ModeCharDevice != 0
	return historyCmd(ctx, c, args, historyIO{in: os.Stdin, out: os.Stdout, err: os.Stderr, tty: tty})
}

func historyCmd(ctx context.Context, c *api.Client, args []string, io historyIO) error {
	if len(args) == 0 {
		return errors.New(strings.TrimRight(historyUsage, "\n"))
	}
	h := &hist{c: c, io: io}
	switch cmd, rest := args[0], args[1:]; cmd {
	case "list", "ls":
		return h.list(ctx, rest)
	case "send":
		fs := flags("send", io)
		to := fs.String("to", "all", "target device or all")
		request := fs.String("request-id", "", "idempotency key")
		asJSON := fs.Bool("json", false, "print submission result")
		pos, err := parse(fs, rest)
		if err != nil {
			return err
		}
		if len(pos) != 1 {
			return fmt.Errorf("usage: unic history send REF --to DEVICE|all")
		}
		item, err := h.resolve(ctx, pos[0])
		if err != nil {
			return err
		}
		if *request == "" {
			*request = rand.Text()
		}
		var devices []string
		if *to != "all" {
			id, err := resolveDevice(ctx, c, *to)
			if err != nil {
				return err
			}
			devices = []string{id}
		}
		connectOpportunity(ctx, c)
		out, err := c.Submit(ctx, api.SubmitInput{HistoryID: item.ID, RequestID: *request, DeviceIDs: devices, Async: true})
		if err != nil {
			return err
		}
		if out.Phase == engine.SubmissionProcessing {
			out, err = c.WaitSubmission(ctx, *request, nil)
			if err != nil {
				return err
			}
		}
		outcome, op, err := awaitSend(ctx, c, out.OperationID)
		if err != nil {
			return err
		}
		if *asJSON {
			if err := json.NewEncoder(io.out).Encode(submitReport{Submission: out, Send: sendReport{Outcome: outcome, Operation: op}}); err != nil {
				return err
			}
		} else {
			fmt.Fprintln(io.out, "send="+outcome)
		}
		return sendIncomplete(outcome, *request)
	case "export":
		fs := flags("export", io)
		output := fs.String("output", "", "output directory")
		pos, e := parse(fs, rest)
		if e != nil {
			return e
		}
		if len(pos) != 1 || *output == "" {
			return fmt.Errorf("usage: unic history export ID --output DIR")
		}
		it, e := h.resolve(ctx, pos[0])
		if e != nil {
			return e
		}
		names, e := h.c.ExportHistory(ctx, it.ID, *output)
		if e != nil {
			return e
		}
		return json.NewEncoder(io.out).Encode(names)
	case "show":
		return h.show(ctx, rest)
	case "use":
		fs := flags("use", io)
		request := fs.String("request-id", "", "idempotency key (30 minute window)")
		asJSON := fs.Bool("json", false, "print local and network results separately")
		pos, err := parse(fs, rest)
		if err != nil {
			return err
		}
		if len(pos) != 1 {
			return fmt.Errorf("usage: unic history use <ref> [--request-id KEY] [--json]")
		}
		it, err := h.resolve(ctx, pos[0])
		if err != nil {
			return err
		}
		if *request == "" {
			*request = rand.Text()
		}
		connectOpportunity(ctx, h.c)
		result, err := h.c.RestoreHistory(ctx, it.ID, *request)
		if err != nil {
			return err
		}
		outcome, op, err := awaitSend(ctx, h.c, result.OperationID)
		if err != nil {
			return err
		}
		if *asJSON {
			if err := json.NewEncoder(io.out).Encode(map[string]any{"restore": result, "send": sendReport{Outcome: outcome, Operation: op}}); err != nil {
				return err
			}
		} else if _, err = fmt.Fprintf(io.out, "local_applied=%t sync=%s operation=%s send=%s\n", result.LocalApplied, result.SyncDisposition, result.OperationID, outcome); err != nil {
			return err
		}
		return sendIncomplete(outcome, *request)
	case "delete", "rm":
		return h.delete(ctx, rest)
	case "tag":
		return h.tag(ctx, rest)
	case "expire":
		return h.expire(ctx, rest)
	case "action":
		return h.action(ctx, rest)
	case "help", "-h", "--help":
		_, _ = fmt.Fprint(io.out, historyUsage)
		return nil
	}
	return fmt.Errorf("unknown history command %q\n%s", args[0], strings.TrimRight(historyUsage, "\n"))
}

type hist struct {
	c  *api.Client
	io historyIO
}

func flags(name string, io historyIO) *flag.FlagSet {
	fs := flag.NewFlagSet("history "+name, flag.ContinueOnError)
	fs.SetOutput(io.err)
	return fs
}

// parse lets flags come before or after the positional arguments.
func parse(fs *flag.FlagSet, args []string) ([]string, error) {
	var pos []string
	for {
		if err := fs.Parse(args); err != nil {
			return nil, err
		}
		if fs.NArg() == 0 {
			return pos, nil
		}
		pos = append(pos, fs.Arg(0))
		args = fs.Args()[1:]
	}
}

const maxPage = 200

// resolve turns an id or @N into the entry.
func (h *hist) resolve(ctx context.Context, ref string) (api.HistoryItem, error) {
	if !strings.HasPrefix(ref, "@") {
		return h.c.GetHistory(ctx, ref)
	}
	n, err := strconv.Atoi(ref[1:])
	if err != nil || n < 1 || n > maxPage {
		return api.HistoryItem{}, fmt.Errorf("%q: use @1 to @%d", ref, maxPage)
	}
	page, err := h.c.QueryHistory(ctx, api.HistoryQuery{Limit: n})
	if err != nil {
		return api.HistoryItem{}, err
	}
	if len(page.Items) < n {
		return api.HistoryItem{}, fmt.Errorf("%s: the history has only %d entries", ref, len(page.Items))
	}
	return page.Items[n-1], nil
}

func split(s string) []string {
	var out []string
	for _, p := range strings.Split(s, ",") {
		if p = strings.TrimSpace(p); p != "" {
			out = append(out, p)
		}
	}
	return out
}

func (h *hist) list(ctx context.Context, args []string) error {
	fs := flags("list", h.io)
	kinds := fs.String("type", "", "text,image,files")
	origins := fs.String("origin", "", "origins, comma separated")
	tag := fs.String("tag", "", "only entries with this tag")
	limit := fs.Int("limit", 20, "how many entries")
	fs.IntVar(limit, "n", 20, "how many entries")
	asJSON := fs.Bool("json", false, "print JSON")
	pos, err := parse(fs, args)
	if err != nil {
		return err
	}
	if len(pos) > 0 {
		return errors.New("usage: unic history list [flags]")
	}
	if *limit < 1 {
		return errors.New("limit must be at least 1")
	}
	q := api.HistoryQuery{Kinds: split(*kinds), Origins: split(*origins)}
	if *tag != "" {
		q.Tags = []string{*tag}
	}
	var items []api.HistoryItem
	for len(items) < *limit {
		q.Limit = min(*limit-len(items), maxPage)
		page, err := h.c.QueryHistory(ctx, q)
		if err != nil {
			return err
		}
		items = append(items, page.Items...)
		if page.NextCursor == "" || len(page.Items) == 0 {
			break
		}
		q.Cursor = page.NextCursor
	}
	if *asJSON {
		if items == nil {
			items = []api.HistoryItem{}
		}
		return json.NewEncoder(h.io.out).Encode(items)
	}
	if len(items) == 0 {
		_, _ = fmt.Fprintln(h.io.err, "no entries")
		return nil
	}
	w := tabwriter.NewWriter(h.io.out, 0, 0, 2, ' ', 0)
	_, _ = fmt.Fprintln(w, "REF\tID\tTIME\tTYPE\tFROM\tSIZE\tTAGS")
	for i, it := range items {
		_, _ = fmt.Fprintf(w, "@%d\t%s\t%s\t%s\t%s\t%s\t%s\n", i+1, it.ID, time.UnixMilli(it.CreatedAt).Format("2006-01-02 15:04:05"),
			it.Kind, from(it), size(it.TotalBytes), strings.Join(it.Tags, ","))
	}
	return w.Flush()
}

func from(it api.HistoryItem) string {
	switch {
	case it.DeviceName != "":
		return it.DeviceName
	case it.PluginID != "":
		return it.Origin + ":" + it.PluginID
	}
	return it.Origin
}

func size(n int64) string {
	switch {
	case n >= 1<<20:
		return fmt.Sprintf("%.1fMB", float64(n)/(1<<20))
	case n >= 1<<10:
		return fmt.Sprintf("%.1fKB", float64(n)/(1<<10))
	}
	return fmt.Sprintf("%dB", n)
}

func (h *hist) show(ctx context.Context, args []string) error {
	fs := flags("show", h.io)
	part := fs.Int("part", -1, "part index")
	asJSON := fs.Bool("json", false, "print the entry's details")
	pos, err := parse(fs, args)
	if err != nil {
		return err
	}
	ref := "@1"
	switch len(pos) {
	case 1:
		ref = pos[0]
	case 0:
	default:
		return errors.New("usage: unic history show [ref] [--part N] [--json]")
	}
	it, err := h.resolve(ctx, ref)
	if err != nil {
		return err
	}
	if *asJSON {
		return json.NewEncoder(h.io.out).Encode(it)
	}
	if *part < 0 {
		for i, p := range it.Parts {
			if p.MIME == "text/plain" {
				*part = i
				break
			}
		}
	}
	if *part < 0 {
		var kinds []string
		for i, p := range it.Parts {
			kinds = append(kinds, fmt.Sprintf("%d=%s", i, firstNonEmpty(p.MIME, p.Kind)))
		}
		return fmt.Errorf("this entry has no text part; pick one with --part (%s)", strings.Join(kinds, ", "))
	}
	if *part >= len(it.Parts) {
		return fmt.Errorf("part %d: the entry has %d parts", *part, len(it.Parts))
	}
	return h.c.CopyHistoryContent(ctx, it.ID, *part, h.io.out)
}

func firstNonEmpty(a, b string) string {
	if a != "" {
		return a
	}
	return b
}

func (h *hist) delete(ctx context.Context, args []string) error {
	fs := flags("delete", h.io)
	yes := fs.Bool("y", false, "do not ask")
	fs.BoolVar(yes, "yes", false, "do not ask")
	refs, err := parse(fs, args)
	if err != nil {
		return err
	}
	if len(refs) == 0 {
		return errors.New("usage: unic history delete <ref>... [-y]")
	}
	// Resolve everything first, so @N means the same entry for each argument.
	var items []api.HistoryItem
	seen := map[string]bool{}
	for _, r := range refs {
		it, err := h.resolve(ctx, r)
		if err != nil {
			return err
		}
		if !seen[it.ID] {
			seen[it.ID] = true
			items = append(items, it)
		}
	}
	if h.io.tty && !*yes {
		_, _ = fmt.Fprintf(h.io.err, "Delete %d entr%s? [y/N] ", len(items), map[bool]string{true: "y", false: "ies"}[len(items) == 1])
		line, _ := bufio.NewReader(h.io.in).ReadString('\n')
		if a := strings.ToLower(strings.TrimSpace(line)); a != "y" && a != "yes" {
			_, _ = fmt.Fprintln(h.io.err, "cancelled")
			return nil
		}
	}
	for _, it := range items {
		if err := h.c.DeleteHistory(ctx, it.ID, it.Revision); err != nil {
			return fmt.Errorf("%s: %w", it.ID, err)
		}
		_, _ = fmt.Fprintln(h.io.out, "deleted", it.ID)
	}
	return nil
}

func (h *hist) tag(ctx context.Context, args []string) error {
	fs := flags("tag", h.io)
	add := fs.String("add", "", "tags to add")
	rm := fs.String("remove", "", "tags to remove")
	set := fs.String("set", "", "replace all tags")
	pos, err := parse(fs, args)
	if err != nil {
		return err
	}
	if len(pos) != 1 || (*add == "" && *rm == "" && !flagSet(fs, "set")) || (flagSet(fs, "set") && (*add != "" || *rm != "")) {
		return errors.New("usage: unic history tag <ref> [--add a,b] [--remove c] | --set a,b")
	}
	it, err := h.resolve(ctx, pos[0])
	if err != nil {
		return err
	}
	tags := it.Tags
	if flagSet(fs, "set") {
		tags = split(*set)
	} else {
		drop := map[string]bool{}
		for _, t := range split(*rm) {
			drop[t] = true
		}
		var next []string
		have := map[string]bool{}
		for _, t := range append(append([]string{}, tags...), split(*add)...) {
			if !drop[t] && !have[t] {
				have[t] = true
				next = append(next, t)
			}
		}
		tags = next
	}
	if tags == nil {
		tags = []string{}
	}
	out, err := h.c.UpdateHistory(ctx, it.ID, api.HistoryUpdate{Revision: it.Revision, Tags: &tags})
	if err != nil {
		return err
	}
	_, _ = fmt.Fprintf(h.io.out, "%s  tags: %s\n", out.ID, strings.Join(out.Tags, ","))
	return nil
}

func flagSet(fs *flag.FlagSet, name string) bool {
	set := false
	fs.Visit(func(f *flag.Flag) { set = set || f.Name == name })
	return set
}

func (h *hist) expire(ctx context.Context, args []string) error {
	if len(args) != 2 {
		return errors.New("usage: unic history expire <ref> <duration|never>")
	}
	var at int64
	if args[1] != "never" {
		d, err := parseDuration(args[1])
		if err != nil {
			return err
		}
		at = time.Now().Add(d).UnixMilli()
	}
	it, err := h.resolve(ctx, args[0])
	if err != nil {
		return err
	}
	out, err := h.c.UpdateHistory(ctx, it.ID, api.HistoryUpdate{Revision: it.Revision, ExpiresAtMs: &at})
	if err != nil {
		return err
	}
	if out.ExpiresAt == 0 {
		_, _ = fmt.Fprintf(h.io.out, "%s  never expires\n", out.ID)
	} else {
		_, _ = fmt.Fprintf(h.io.out, "%s  expires %s\n", out.ID, time.UnixMilli(out.ExpiresAt).Format("2006-01-02 15:04:05"))
	}
	return nil
}

// parseDuration is time.ParseDuration plus a "d" suffix for days.
func parseDuration(s string) (time.Duration, error) {
	if n, ok := strings.CutSuffix(s, "d"); ok {
		days, err := strconv.Atoi(n)
		if err != nil || days < 1 {
			return 0, fmt.Errorf("%q: not a duration (try 24h, 7d or never)", s)
		}
		return time.Duration(days) * 24 * time.Hour, nil
	}
	d, err := time.ParseDuration(s)
	if err != nil || d <= 0 {
		return 0, fmt.Errorf("%q: not a duration (try 24h, 7d or never)", s)
	}
	return d, nil
}

func (h *hist) action(ctx context.Context, args []string) error {
	fs := flags("action", h.io)
	apply := fs.Bool("apply", false, "apply the committed result to the system clipboard")
	request := fs.String("request-id", "", "idempotency key")
	pos, err := parse(fs, args)
	if err != nil {
		return err
	}
	if len(pos) == 1 {
		acts, err := h.c.Actions(ctx)
		if err != nil {
			return err
		}
		for _, a := range acts {
			_, _ = fmt.Fprintf(h.io.out, "%s\t%s\n", a.Key, a.Title)
		}
		return nil
	}
	if len(pos) != 2 {
		return fmt.Errorf("usage: unic history action ID [KEY] [--apply] [--request-id KEY]")
	}
	it, err := h.resolve(ctx, pos[0])
	if err != nil {
		return err
	}
	if *request == "" {
		*request = rand.Text()
	}
	_, _ = fmt.Fprintf(h.io.err, "request_id=%s\n", *request)
	connectOpportunity(ctx, h.c)
	out, err := h.c.ExecuteAction(ctx, it.ID, pos[1], api.ActionInput{RequestID: *request, Apply: *apply})
	if err != nil {
		return err
	}
	outcome, op, err := awaitSend(ctx, h.c, out.Submission.OperationID)
	if err != nil {
		return err
	}
	if err := json.NewEncoder(h.io.out).Encode(struct {
		api.ActionOutput
		Send sendReport `json:"send"`
	}{out, sendReport{Outcome: outcome, Operation: op}}); err != nil {
		return err
	}
	if out.ApplyError != nil {
		return out.ApplyError
	}
	return sendIncomplete(outcome, *request)
}
