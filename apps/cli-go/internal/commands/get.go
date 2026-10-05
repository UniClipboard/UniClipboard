package commands

import (
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
	"unicode"

	"golang.org/x/term"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// getDefaultLimit is the number of recent entries scanned when selecting or
// listing.
const getDefaultLimit = 50

// getCategory is the classified kind of a history entry.
type getCategory string

const (
	categoryImage getCategory = "image"
	categoryFile  getCategory = "file"
	categoryText  getCategory = "text"
	categoryLink  getCategory = "link"
)

type getArgs struct {
	kind  getCategory // empty when --type is absent
	id    *string
	list  bool
	limit *uint64
	out   *string
	copy  bool
	wait  bool
}

// entryProjection is the subset of `EntryProjectionResponseDto` the command
// reads.
type entryProjection struct {
	ID           string    `json:"id"`
	Preview      string    `json:"preview"`
	CapturedAt   int64     `json:"capturedAt"`
	ContentType  string    `json:"contentType"`
	LinkURLs     *[]string `json:"linkUrls"`
	PayloadState *string   `json:"payloadState"`
}

// getOutcome mirrors the Rust `GetOutcome` JSON shape.
type getOutcome struct {
	EntryID      string  `json:"entry_id"`
	ContentType  string  `json:"content_type"`
	MimeType     string  `json:"mime_type"`
	Path         *string `json:"path"`
	Filename     *string `json:"filename"`
	Text         *string `json:"text"`
	BytesWritten *uint64 `json:"bytes_written"`
	CapturedAt   int64   `json:"captured_at"`
	Outcome      string  `json:"outcome"`
}

// getListRow mirrors the Rust `ListRow` JSON shape.
type getListRow struct {
	EntryID     string `json:"entry_id"`
	ContentType string `json:"content_type"`
	MimeType    string `json:"mime_type"`
	CapturedAt  int64  `json:"captured_at"`
	Preview     string `json:"preview"`
	Lost        bool   `json:"lost"`
}

func runGet(ctx *cli.Context) int {
	args := getArgs{kind: getCategory(ctx.String("type")), list: ctx.Bool("list"), copy: ctx.Bool("copy"), wait: ctx.Bool("wait")}
	if ctx.Has("id") {
		id := ctx.String("id")
		args.id = &id
	}
	if ctx.Has("limit") {
		limit := ctx.Uint("limit")
		args.limit = &limit
	}
	if ctx.Has("out") {
		out := ctx.String("out")
		args.out = &out
	}
	json := ctx.JSON()

	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	if args.wait {
		return runGetWait(client, lease, &args, json)
	}
	defer lease.Release()

	limit := uint64(getDefaultLimit)
	if args.limit != nil {
		limit = max(*args.limit, 1)
	}
	entries, err := listEntries(client, limit, 0)
	if err != nil {
		ui.Error("Failed to list clipboard entries: " + err.Error())
		return exitcode.Error
	}
	if args.list {
		return printGetList(entries, json)
	}
	target, code := selectGetTarget(entries, &args)
	if target == nil {
		return code
	}
	return materializeEntry(client, target, &args, json)
}

func listEntries(client *daemonclient.Client, limit, offset uint64) ([]entryProjection, error) {
	// The query is part of the path, as in the Rust client, so error text
	// names the full route.
	path := fmt.Sprintf("/clipboard/entries?limit=%d&offset=%d", limit, offset)
	var entries []entryProjection
	if err := client.Get(context.Background(), path, &entries); err != nil {
		return nil, err
	}
	return entries, nil
}

func runGetWait(client *daemonclient.Client, lease *daemonclient.Lease, args *getArgs, json bool) int {
	wait, code := connectInboundWait(client, lease, waitActivity)
	if wait == nil {
		return code
	}
	defer wait.close()
	display := newWaitDisplay(json)
	defer display.clear()

	type waitTarget struct {
		entryID   string
		attemptID *string
	}
	matches := func(t *waitTarget, entryID, attemptID *string) bool {
		return entryID != nil && *entryID == t.entryID &&
			(t.attemptID == nil || (attemptID != nil && *attemptID == *t.attemptID))
	}
	var target *waitTarget

	for {
		event, code := wait.nextActivity()
		if event == nil {
			display.clear()
			return code
		}
		switch ev := event.(type) {
		case activityReconnected:
			target = nil
			display.waiting()
		case activityPending:
			if target == nil && pendingCanMatch(ev, args) {
				target = &waitTarget{entryID: ev.EntryID, attemptID: ev.AttemptID}
			}
		case activityProgress:
			if ev.Receiving && target != nil && matches(target, ev.EntryID, ev.AttemptID) {
				display.progress(ev.BytesTransferred, ev.TotalBytes)
			}
		case activityStatus:
			if target != nil && matches(target, ev.EntryID, ev.AttemptID) && (ev.Status == "failed" || ev.Status == "cancelled") {
				display.clear()
				reason := "no reason reported"
				if ev.Reason != nil {
					reason = *ev.Reason
				}
				ui.Error(fmt.Sprintf("Incoming transfer %s: %s", ev.Status, reason))
				return exitcode.Error
			}
		case activityCompleted:
			if target != nil && target.entryID != ev.EntryID {
				continue
			}
			entry, err := findEntry(wait.client, ev.EntryID)
			if err != nil {
				display.clear()
				ui.Error("Failed to read the synced entry: " + err.Error())
				return exitcode.Error
			}
			if entry == nil {
				display.clear()
				ui.Error("The synced entry arrived but could not be read from history.")
				return exitcode.Error
			}
			if !entryMatchesArgs(entry, args) {
				if target != nil && target.entryID == entry.ID {
					target = nil
					display.waiting()
				}
				continue
			}
			display.clear()
			return materializeEntry(wait.client, entry, args, json)
		}
	}
}

func pendingCanMatch(pending activityPending, args *getArgs) bool {
	if args.id != nil {
		return pending.EntryID == *args.id
	}
	switch args.kind {
	case "":
		return true
	case categoryFile:
		return len(pending.Filenames) > 0
	default:
		return false
	}
}

func entryMatchesArgs(entry *entryProjection, args *getArgs) bool {
	return (args.id == nil || entry.ID == *args.id) &&
		(args.kind == "" || classifyEntry(entry) == args.kind) &&
		!entryIsLost(entry)
}

func findEntry(client *daemonclient.Client, entryID string) (*entryProjection, error) {
	offset := uint64(0)
	for {
		entries, err := listEntries(client, getDefaultLimit, offset)
		if err != nil {
			return nil, err
		}
		for i := range entries {
			if entries[i].ID == entryID {
				return &entries[i], nil
			}
		}
		if len(entries) < getDefaultLimit {
			return nil, nil
		}
		offset += uint64(len(entries))
	}
}

// waitDisplay renders `get --wait` progress on an interactive stderr.
type waitDisplay struct {
	interactive bool
	bar         *ui.Spinner
	hasLength   bool
	length      uint64
}

func newWaitDisplay(json bool) *waitDisplay {
	d := &waitDisplay{interactive: !json && ui.StderrIsTerminal()}
	if d.interactive {
		d.bar = ui.NewSpinner("Waiting for the next synced entry — press Ctrl-C to stop")
	} else if !json {
		ui.Info("status", "Waiting for the next synced entry — press Ctrl-C to stop")
	}
	return d
}

func (d *waitDisplay) waiting() {
	if d.interactive {
		d.clear()
		d.bar = ui.NewSpinner("Waiting for the next matching synced entry — press Ctrl-C to stop")
		d.hasLength = false
	}
}

func (d *waitDisplay) progress(completed uint64, total *uint64) {
	if !d.interactive {
		return
	}
	var replace bool
	switch {
	case d.bar == nil:
		replace = true
	case total != nil:
		replace = !d.hasLength || d.length != *total
	default:
		replace = d.hasLength
	}
	if replace {
		d.clear()
		if total != nil {
			d.bar = ui.NewByteProgress(*total, "Receiving")
			d.hasLength, d.length = true, *total
		} else {
			d.bar = ui.NewSpinner("Receiving")
			d.hasLength = false
		}
	}
	if total != nil {
		d.bar.SetLength(*total)
		d.bar.SetPosition(min(completed, *total))
	} else {
		d.bar.SetMessage("Receiving " + getHumanSize(completed))
	}
}

func (d *waitDisplay) clear() {
	if d.bar != nil {
		d.bar.Clear()
		d.bar = nil
	}
}

func getHumanSize(bytes uint64) string {
	units := []string{"B", "KiB", "MiB", "GiB", "TiB"}
	value := float64(bytes)
	unit := 0
	for value >= 1024 && unit < len(units)-1 {
		value /= 1024
		unit++
	}
	if unit == 0 {
		return fmt.Sprintf("%d %s", bytes, units[unit])
	}
	return fmt.Sprintf("%.1f %s", value, units[unit])
}

// selectGetTarget picks the entry to materialize: the exact `--id`, or the
// newest usable (non-Lost) match. A nil entry carries the exit code.
func selectGetTarget(entries []entryProjection, args *getArgs) (*entryProjection, int) {
	if args.id != nil {
		for i := range entries {
			if entries[i].ID != *args.id {
				continue
			}
			if entryIsLost(&entries[i]) {
				ui.Error(fmt.Sprintf("Entry %s exists but its payload is no longer available (Lost). Re-send it from the source device.", shortEntryID(*args.id)))
				return nil, exitcode.ContentUnavailable
			}
			return &entries[i], exitcode.Success
		}
		ui.Error(fmt.Sprintf("No entry with id %s in the latest %d entries. It may be older — raise --limit, or find it with `uniclip search`.", shortEntryID(*args.id), len(entries)))
		return nil, exitcode.NoMatch
	}
	for i := range entries {
		if !entryIsLost(&entries[i]) && (args.kind == "" || classifyEntry(&entries[i]) == args.kind) {
			return &entries[i], exitcode.Success
		}
	}
	what := "usable entry"
	if args.kind != "" {
		what = string(args.kind) + " entry"
	}
	ui.Error(fmt.Sprintf("No %s found in the latest %d entries.", what, len(entries)))
	return nil, exitcode.NoMatch
}

func materializeEntry(client *daemonclient.Client, target *entryProjection, args *getArgs, json bool) int {
	switch category := classifyEntry(target); category {
	case categoryFile:
		return emitFile(client, target, args, json)
	case categoryImage:
		return emitImage(client, target, args, json)
	default:
		return emitText(client, target, category, args, json)
	}
}

func entryRoute(entryID, suffix string) (string, error) {
	segment, err := daemonclient.PathSegment(entryID)
	if err != nil {
		return "", err
	}
	return "/clipboard/entries/" + segment + suffix, nil
}

func emitText(client *daemonclient.Client, target *entryProjection, category getCategory, args *getArgs, json bool) int {
	var detail struct {
		Content string `json:"content"`
	}
	path, err := entryRoute(target.ID, "")
	if err == nil {
		err = client.Get(context.Background(), path, &detail)
	}
	if daemonclient.IsNotFound(err) {
		return contentUnavailable(target.ID)
	}
	if err != nil {
		ui.Error("Failed to read entry text: " + err.Error())
		return exitcode.Error
	}

	if args.copy {
		if err := copyToTerminal(detail.Content); err != nil {
			ui.Error(err.Error())
			return exitcode.Error
		}
	}

	if json {
		output.EmitJSON(getOutcome{
			EntryID:     target.ID,
			ContentType: string(category),
			MimeType:    target.ContentType,
			Text:        &detail.Content,
			CapturedAt:  target.CapturedAt,
			Outcome:     "exported",
		}, "get outcome")
		return exitcode.Success
	}
	// Raw content: add a trailing newline only for an interactive stdout so
	// piped output keeps the exact bytes.
	os.Stdout.WriteString(detail.Content)
	if term.IsTerminal(int(os.Stdout.Fd())) && !strings.HasSuffix(detail.Content, "\n") {
		os.Stdout.WriteString("\n")
	}
	return exitcode.Success
}

func emitFile(client *daemonclient.Client, target *entryProjection, args *getArgs, json bool) int {
	filename, bytes, found, err := exportEntryFile(client, target.ID)
	if err != nil {
		ui.Error("Failed to export file: " + err.Error())
		return exitcode.Error
	}
	if !found {
		return contentUnavailable(target.ID)
	}
	return writeBytesOutcome(target, categoryFile, sanitizeGetFilename(filename), bytes, args, json)
}

// exportEntryFile calls `GET /clipboard/entries/{id}/file`; found=false on 404.
func exportEntryFile(client *daemonclient.Client, entryID string) (string, []byte, bool, error) {
	path, err := entryRoute(entryID, "/file")
	if err != nil {
		return "", nil, false, err
	}
	resp, err := client.Send(context.Background(), daemonclient.Request{Method: http.MethodGet, Path: path})
	if daemonclient.IsNotFound(err) {
		return "", nil, false, nil
	}
	if err != nil {
		return "", nil, false, err
	}
	defer resp.Body.Close()
	filename, ok := filenameFromContentDisposition(resp.Header.Get("Content-Disposition"))
	if !ok {
		filename = entryID
	}
	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", nil, false, &daemonclient.RequestError{Kind: daemonclient.ErrDecode, Path: path, Err: errors.New("error decoding response body")}
	}
	return filename, data, true, nil
}

// filenameFromContentDisposition extracts `filename="..."` (or a bare
// `filename=...`). Like reqwest's `HeaderValue::to_str`, a header with
// non-visible-ASCII bytes is unusable.
func filenameFromContentDisposition(header string) (string, bool) {
	for i := 0; i < len(header); i++ {
		if c := header[i]; c != '\t' && (c < 32 || c > 126) {
			return "", false
		}
	}
	for _, part := range strings.Split(header, ";") {
		part = strings.TrimSpace(part)
		if rest, ok := strings.CutPrefix(part, "filename="); ok {
			if value := strings.Trim(strings.TrimSpace(rest), `"`); value != "" {
				return value, true
			}
		}
	}
	return "", false
}

func emitImage(client *daemonclient.Client, target *entryProjection, args *getArgs, json bool) int {
	var resource struct {
		BlobID     *string `json:"blobId"`
		MimeType   *string `json:"mimeType"`
		InlineData *string `json:"inlineData"`
	}
	path, err := entryRoute(target.ID, "/resource")
	if err == nil {
		err = client.Get(context.Background(), path, &resource)
	}
	if daemonclient.IsNotFound(err) {
		return contentUnavailable(target.ID)
	}
	if err != nil {
		ui.Error("Failed to read image resource: " + err.Error())
		return exitcode.Error
	}
	mime := target.ContentType
	if resource.MimeType != nil {
		mime = *resource.MimeType
	}

	// Small images are stored inline (base64); larger ones live in a blob.
	var bytes []byte
	switch {
	case resource.InlineData != nil:
		bytes, err = base64.StdEncoding.DecodeString(*resource.InlineData)
		if err != nil {
			ui.Error("Failed to decode inline image data: " + err.Error())
			return exitcode.Error
		}
	case resource.BlobID != nil:
		var found bool
		bytes, found, err = fetchBlob(client, *resource.BlobID)
		if err != nil {
			ui.Error("Failed to fetch image blob: " + err.Error())
			return exitcode.Error
		}
		if !found {
			return contentUnavailable(target.ID)
		}
	default:
		return contentUnavailable(target.ID)
	}
	filename := fmt.Sprintf("clip-%s.%s", shortEntryID(target.ID), extFromMime(mime))
	return writeBytesOutcome(target, categoryImage, filename, bytes, args, json)
}

// fetchBlob calls `GET /clipboard/blobs/{id}`; found=false on 404.
func fetchBlob(client *daemonclient.Client, blobID string) ([]byte, bool, error) {
	segment, err := daemonclient.PathSegment(blobID)
	if err != nil {
		return nil, false, err
	}
	path := "/clipboard/blobs/" + segment
	resp, err := client.Send(context.Background(), daemonclient.Request{Method: http.MethodGet, Path: path})
	if daemonclient.IsNotFound(err) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, false, &daemonclient.RequestError{Kind: daemonclient.ErrDecode, Path: path, Err: errors.New("error decoding response body")}
	}
	return data, true, nil
}

// writeBytesOutcome sends image/file bytes to stdout (`--out -`) or to a
// file in the resolved output directory.
func writeBytesOutcome(target *entryProjection, category getCategory, filename string, bytes []byte, args *getArgs, json bool) int {
	bytesWritten := uint64(len(bytes))

	if args.out != nil && *args.out == "-" {
		if args.copy {
			ui.Error("--copy cannot be combined with --out - for image or file content because no file path is produced")
			return exitcode.Error
		}
		if json {
			ui.Error("--json cannot be combined with --out - because the JSON would corrupt the raw byte stream on stdout")
			return exitcode.Error
		}
		// Report a closed reader as a write error (as Rust does) instead of
		// dying from SIGPIPE.
		signal.Ignore(syscall.SIGPIPE)
		if _, err := os.Stdout.Write(bytes); err != nil {
			ui.Error("Failed to write bytes to stdout: " + rustIOError(err))
			return exitcode.Error
		}
		return exitcode.Success
	}

	dir := defaultGetOutDir()
	if args.out != nil {
		dir = *args.out
	}
	outDir, err := ensureOutputDir(dir)
	if err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	targetPath := filepath.Join(outDir, filename)
	if err := os.WriteFile(targetPath, bytes, 0o666); err != nil {
		ui.Error("Failed to write file: " + rustIOError(err))
		return exitcode.Error
	}

	if args.copy {
		if err := copyToTerminal(targetPath); err != nil {
			ui.Error(err.Error())
			return exitcode.Error
		}
	}

	if json {
		output.EmitJSON(getOutcome{
			EntryID:      target.ID,
			ContentType:  string(category),
			MimeType:     target.ContentType,
			Path:         &targetPath,
			Filename:     &filename,
			BytesWritten: &bytesWritten,
			CapturedAt:   target.CapturedAt,
			Outcome:      "exported",
		}, "get outcome")
	} else {
		// Exactly one machine-readable line, safe even when callers merge
		// stdout and stderr.
		fmt.Fprintln(os.Stdout, targetPath)
	}
	return exitcode.Success
}

func printGetList(entries []entryProjection, json bool) int {
	if json {
		rows := make([]getListRow, 0, len(entries))
		for i := range entries {
			e := &entries[i]
			rows = append(rows, getListRow{
				EntryID:     e.ID,
				ContentType: string(classifyEntry(e)),
				MimeType:    e.ContentType,
				CapturedAt:  e.CapturedAt,
				Preview:     e.Preview,
				Lost:        entryIsLost(e),
			})
		}
		if rendered, err := output.Pretty(rows); err == nil {
			fmt.Fprintln(os.Stdout, rendered)
		}
		return exitcode.Success
	}
	if len(entries) == 0 {
		ui.Info("status", "clipboard history is empty")
		return exitcode.Success
	}
	ui.Header("Recent clipboard entries")
	for i := range entries {
		e := &entries[i]
		lost := ""
		if entryIsLost(e) {
			lost = " [lost]"
		}
		ui.Info(shortEntryID(e.ID), fmt.Sprintf("[%s]%s %s", classifyEntry(e), lost, firstLine(e.Preview, 48)))
	}
	ui.End("Done")
	return exitcode.Success
}

func contentUnavailable(entryID string) int {
	ui.Error(fmt.Sprintf("Entry %s has no materialized payload yet (Lost or not downloaded). Re-send it from the source device.", shortEntryID(entryID)))
	return exitcode.ContentUnavailable
}

// classifyEntry maps a projection onto image/file/text/link. Current daemons
// return category labels; older projections may carry MIME values. Links are
// text entries with non-empty link URLs.
func classifyEntry(entry *entryProjection) getCategory {
	contentType := asciiLower(entry.ContentType)
	switch {
	case contentType == "image" || strings.HasPrefix(contentType, "image/"):
		return categoryImage
	case contentType == "file" || contentType == "text/uri-list" || contentType == "file/uri-list":
		return categoryFile
	case entry.LinkURLs != nil && len(*entry.LinkURLs) > 0:
		return categoryLink
	default:
		return categoryText
	}
}

func entryIsLost(entry *entryProjection) bool {
	return entry.PayloadState != nil && asciiLower(*entry.PayloadState) == "lost"
}

func extFromMime(mime string) string {
	switch asciiLower(mime) {
	case "image/png":
		return "png"
	case "image/jpeg", "image/jpg":
		return "jpg"
	case "image/gif":
		return "gif"
	case "image/webp":
		return "webp"
	case "image/bmp":
		return "bmp"
	case "image/tiff":
		return "tiff"
	case "image/svg+xml":
		return "svg"
	default:
		return "bin"
	}
}

// asciiLower lowercases ASCII letters only, like Rust `to_ascii_lowercase`.
func asciiLower(s string) string {
	b := []byte(s)
	for i, c := range b {
		if c >= 'A' && c <= 'Z' {
			b[i] = c + 32
		}
	}
	return string(b)
}

// defaultGetOutDir is `$XDG_CACHE_HOME/uniclip/get`, else
// `$HOME/.cache/uniclip/get`, else a temp-dir fallback.
func defaultGetOutDir() string {
	if xdg := os.Getenv("XDG_CACHE_HOME"); xdg != "" {
		return filepath.Join(xdg, "uniclip", "get")
	}
	if home := os.Getenv("HOME"); home != "" {
		return filepath.Join(home, ".cache", "uniclip", "get")
	}
	return filepath.Join(os.TempDir(), "uniclip-get")
}

// sanitizeGetFilename strips path separators and control characters from a
// remote-supplied filename.
func sanitizeGetFilename(name string) string {
	var b strings.Builder
	for _, r := range name {
		if r == '/' || r == '\\' || unicode.IsControl(r) {
			continue
		}
		b.WriteRune(r)
	}
	stripped := b.String()
	if stripped == "" || stripped == "." || stripped == ".." {
		return "uniclip-get.bin"
	}
	return stripped
}

// shortEntryID is the first eight characters of an id.
func shortEntryID(s string) string {
	count := 0
	for i := range s {
		if count == 8 {
			return s[:i]
		}
		count++
	}
	return s
}

// firstLine returns the first line (Rust `lines()` semantics), truncated to
// max characters with an ellipsis.
func firstLine(s string, max int) string {
	line, _, _ := strings.Cut(s, "\n")
	line = strings.TrimSuffix(line, "\r")
	runes := []rune(line)
	if len(runes) > max {
		return string(runes[:max]) + "…"
	}
	return line
}
