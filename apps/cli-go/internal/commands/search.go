package commands

import (
	"context"
	"fmt"
	"net/http"
	"net/url"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// searchResult is the subset of the daemon's `SearchResultDto` the CLI renders.
type searchResult struct {
	EntryID        string   `json:"entryId"`
	ContentType    string   `json:"contentType"`
	ActiveTimeMs   int64    `json:"activeTimeMs"`
	TextPreview    *string  `json:"textPreview"`
	MimeType       string   `json:"mimeType"`
	FileExtensions []string `json:"fileExtensions"`
	SourceDevice   *string  `json:"sourceDevice"`
}

type searchPage struct {
	Items   []searchResult `json:"items"`
	Total   uint32         `json:"total"`
	HasMore bool           `json:"hasMore"`
}

type searchStatus struct {
	State                    string  `json:"state"`
	Reason                   *string `json:"reason"`
	LastRebuildStartedAtMs   *int64  `json:"lastRebuildStartedAtMs"`
	LastRebuildCompletedAtMs *int64  `json:"lastRebuildCompletedAtMs"`
}

// JSON output shapes keep the CLI's historical snake_case field names.
type searchPageJSON struct {
	Total   uint32             `json:"total"`
	HasMore bool               `json:"has_more"`
	Data    []searchResultJSON `json:"data"`
}

type searchResultJSON struct {
	EntryID        string   `json:"entry_id"`
	ContentType    string   `json:"content_type"`
	ActiveTimeMs   int64    `json:"active_time_ms"`
	TextPreview    *string  `json:"text_preview"`
	MimeType       string   `json:"mime_type"`
	FileExtensions []string `json:"file_extensions"`
	SourceDevice   *string  `json:"source_device"`
}

type searchStatusJSON struct {
	State                    string  `json:"state"`
	Reason                   *string `json:"reason"`
	LastRebuildStartedAtMs   *int64  `json:"last_rebuild_started_at_ms"`
	LastRebuildCompletedAtMs *int64  `json:"last_rebuild_completed_at_ms"`
}

type searchRebuildJSON struct {
	Accepted bool             `json:"accepted"`
	Status   searchStatusJSON `json:"status"`
}

type searchErrorJSON struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func runSearch(ctx *cli.Context) int {
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	bg := context.Background()

	// Resolve `--source-device` before anything else so an unknown device
	// fails fast and a source-only browse still counts as a filter.
	sourceDevices, code := resolveSourceDevices(bg, client, ctx.Strings("source-device"))
	if code != exitcode.Success {
		return code
	}

	contentTypes, tags, extensions := ctx.Strings("type"), ctx.Strings("tag"), ctx.Strings("ext")
	hasFrom, hasTo := ctx.Has("from-ms"), ctx.Has("to-ms")
	hasFilter := len(tags) > 0 || len(contentTypes) > 0 || len(extensions) > 0 ||
		len(sourceDevices) > 0 || hasFrom || hasTo
	queryString, hasQuery := ctx.Arg(0)
	if !hasQuery && !hasFilter {
		ui.Error("Missing search query. Run `uniclip search <query>`, narrow with `--tag`/`--type`/`--ext`, or `search status` / `search rebuild`.")
		return exitcode.Error
	}
	if hasFrom != hasTo {
		ui.Error("--from-ms and --to-ms must be provided together")
		return exitcode.Error
	}

	params := url.Values{}
	params.Set("query", queryString)
	params.Set("limit", strconv.FormatUint(ctx.Uint("limit"), 10))
	params.Set("offset", strconv.FormatUint(ctx.Uint("offset"), 10))
	if ctx.Has("operator") {
		params.Set("operator", ctx.String("operator"))
	}
	if ctx.Has("time-preset") {
		params.Set("timePreset", ctx.String("time-preset"))
	}
	if hasFrom {
		params.Set("fromMs", canonicalInt(ctx.String("from-ms")))
		params.Set("toMs", canonicalInt(ctx.String("to-ms")))
	}
	for key, values := range map[string][]string{
		"contentTypes": contentTypes, "tags": tags, "extensions": extensions, "sourceDevices": sourceDevices,
	} {
		if len(values) > 0 {
			params.Set(key, strings.Join(values, ","))
		}
	}

	var page searchPage
	req := daemonclient.Request{Method: http.MethodGet, Path: "/search/query", Query: params}
	if err := client.Enveloped(bg, req, &page); err != nil {
		return renderSearchError("query search index", err, ctx.JSON())
	}

	if ctx.JSON() {
		out := searchPageJSON{Total: page.Total, HasMore: page.HasMore, Data: []searchResultJSON{}}
		for _, item := range page.Items {
			out.Data = append(out.Data, searchResultJSON{
				EntryID: item.EntryID, ContentType: item.ContentType, ActiveTimeMs: item.ActiveTimeMs,
				TextPreview: item.TextPreview, MimeType: item.MimeType,
				FileExtensions: nonNil(item.FileExtensions), SourceDevice: item.SourceDevice,
			})
		}
		return output.EmitJSON(out, "search query response")
	}
	fmt.Fprintln(os.Stdout, renderSearchQuery(page, ctx.Bool("detailed")))
	return exitcode.Success
}

func runSearchStatus(ctx *cli.Context) int {
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()

	var status searchStatus
	if err := client.Get(context.Background(), "/search/status", &status); err != nil {
		return renderSearchError("get search status", err, ctx.JSON())
	}
	if ctx.JSON() {
		return output.EmitJSON(status.toJSON(), "search status response")
	}
	fmt.Fprintln(os.Stdout, renderSearchStatus(status))
	return exitcode.Success
}

func runSearchRebuild(ctx *cli.Context) int {
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	bg := context.Background()

	if err := client.Enveloped(bg, daemonclient.Request{Method: http.MethodPost, Path: "/search/rebuild"}, nil); err != nil {
		return renderSearchError("rebuild search index", err, ctx.JSON())
	}
	var status searchStatus
	if err := client.Get(bg, "/search/status", &status); err != nil {
		return renderSearchError("get search status", err, ctx.JSON())
	}
	if ctx.JSON() {
		return output.EmitJSON(searchRebuildJSON{Accepted: true, Status: status.toJSON()}, "search rebuild response")
	}
	fmt.Fprintln(os.Stdout, "Search rebuild accepted (runs in background).")
	fmt.Fprintln(os.Stdout, renderSearchStatus(status))
	return exitcode.Success
}

func (s searchStatus) toJSON() searchStatusJSON {
	return searchStatusJSON{
		State: s.State, Reason: s.Reason,
		LastRebuildStartedAtMs: s.LastRebuildStartedAtMs, LastRebuildCompletedAtMs: s.LastRebuildCompletedAtMs,
	}
}

// canonicalInt re-renders a parsed integer option like Rust's `i64::to_string`.
func canonicalInt(raw string) string {
	n, _ := strconv.ParseInt(raw, 10, 64)
	return strconv.FormatInt(n, 10)
}

func nonNil(values []string) []string {
	if values == nil {
		return []string{}
	}
	return values
}

// renderSearchError reports a failed search call: a compact `{code,message}`
// line on stderr in JSON mode, otherwise a human error line.
func renderSearchError(action string, err error, asJSON bool) int {
	if asJSON {
		dto := searchErrorJSON{Code: "unknown", Message: err.Error()}
		if re, ok := daemonclient.AsRequestError(err); ok && re.Kind == daemonclient.ErrStatus {
			if re.Code != "" {
				dto.Code = re.Code
			}
			dto.Message = re.Message
		}
		if line, err := output.Compact(dto); err == nil {
			ui.RawStderr(line)
		}
	} else if daemonclient.ErrorCode(err) == "session_locked" {
		ui.Error("Search is unavailable while the encryption session is locked. Unlock first, or run `uniclip space status` to inspect application state.")
	} else {
		ui.Error(fmt.Sprintf("Failed to %s: %v", action, err))
	}
	return exitcode.Error
}

// sourceDevice is one device clips can arrive from: its display name and
// the canonical id stored by the search index.
type sourceDevice struct {
	id   string
	name string
}

// fetchSourceDirectory lists the local device, paired peers and mobile-sync
// devices. Each source is best effort: a failing one contributes nothing.
func fetchSourceDirectory(ctx context.Context, client *daemonclient.Client) []sourceDevice {
	var directory []sourceDevice
	var local struct {
		PeerID     string `json:"peerId"`
		DeviceName string `json:"deviceName"`
	}
	if err := client.Get(ctx, "/device/me", &local); err == nil {
		directory = append(directory, sourceDevice{id: local.PeerID, name: local.DeviceName})
	}
	var members []struct {
		PeerID     string `json:"peerId"`
		DeviceName string `json:"deviceName"`
	}
	if err := client.Get(ctx, "/paired-devices", &members); err == nil {
		for _, m := range members {
			directory = append(directory, sourceDevice{id: m.PeerID, name: m.DeviceName})
		}
	}
	var mobiles []struct {
		DeviceID string `json:"deviceId"`
		Label    string `json:"label"`
	}
	if err := client.Get(ctx, "/mobile-sync/devices", &mobiles); err == nil {
		for _, d := range mobiles {
			directory = append(directory, sourceDevice{id: "mobile_sync:" + d.DeviceID, name: d.Label})
		}
	}
	return directory
}

// resolveSourceDevices maps `--source-device` names (case-insensitive) or
// ids to canonical ids, reporting unknown or ambiguous inputs.
func resolveSourceDevices(ctx context.Context, client *daemonclient.Client, inputs []string) ([]string, int) {
	if len(inputs) == 0 {
		return nil, exitcode.Success
	}
	directory := fetchSourceDirectory(ctx, client)
	var resolved []string
	for _, input := range inputs {
		needle := strings.ToLower(input)
		var nameIDs []string
		for _, entry := range directory {
			if strings.ToLower(entry.name) == needle {
				nameIDs = append(nameIDs, entry.id)
			}
		}
		sort.Strings(nameIDs)
		nameIDs = dedupSorted(nameIDs)

		var id string
		switch len(nameIDs) {
		case 1:
			id = nameIDs[0]
		case 0:
			if !directoryHasID(directory, input) {
				ui.Error(fmt.Sprintf("Unknown source device: '%s'", input))
				renderAvailableSources(directory)
				return nil, exitcode.Error
			}
			id = input
		default:
			ui.Error(fmt.Sprintf("Source device name '%s' is ambiguous; pass an id instead.", input))
			for _, candidate := range nameIDs {
				ui.Info("id", candidate)
			}
			return nil, exitcode.Error
		}
		if !containsString(resolved, id) {
			resolved = append(resolved, id)
		}
	}
	return resolved, exitcode.Success
}

func dedupSorted(values []string) []string {
	out := values[:0]
	for i, v := range values {
		if i == 0 || v != values[i-1] {
			out = append(out, v)
		}
	}
	return out
}

func directoryHasID(directory []sourceDevice, id string) bool {
	for _, entry := range directory {
		if entry.id == id {
			return true
		}
	}
	return false
}

func containsString(values []string, target string) bool {
	for _, v := range values {
		if v == target {
			return true
		}
	}
	return false
}

func renderAvailableSources(directory []sourceDevice) {
	if len(directory) == 0 {
		ui.Info("devices", "no known source devices; run `uniclip member list`")
		return
	}
	ui.Info("devices", "available source devices (name → id):")
	for _, entry := range directory {
		ui.Info(entry.name, entry.id)
	}
}

// chronoMinMs and chronoMaxMs bound the millisecond timestamps chrono's
// `DateTime<Utc>` accepts (years -262144 through 262143).
var (
	chronoMinMs = time.Date(-262144, 1, 1, 0, 0, 0, 0, time.UTC).UnixMilli()
	chronoMaxMs = time.Date(262143, 12, 31, 23, 59, 59, 999_000_000, time.UTC).UnixMilli()
)

func formatSearchTimestamp(ms int64) string {
	if ms < chronoMinMs || ms > chronoMaxMs {
		return fmt.Sprintf("<invalid timestamp: %d>", ms)
	}
	t := time.UnixMilli(ms).UTC()
	year := t.Year()
	yearText := fmt.Sprintf("%04d", year)
	if year < 0 {
		yearText = fmt.Sprintf("-%04d", -year)
	} else if year > 9999 {
		yearText = fmt.Sprintf("+%d", year)
	}
	return yearText + t.Format("-01-02 15:04")
}

func renderSearchQuery(page searchPage, detailed bool) string {
	showingFrom := 0
	if len(page.Items) > 0 {
		showingFrom = 1
	}
	lines := []string{fmt.Sprintf("Search results: %d total (showing %d-%d)", page.Total, showingFrom, len(page.Items))}
	if len(page.Items) == 0 {
		lines = append(lines,
			"No search results found.",
			"Try widening the time range.",
			"Try removing one or more filters.",
			"Try a fuller token; search is exact-token in V1.")
		return strings.Join(lines, "\n")
	}
	for _, item := range page.Items {
		preview := "<no preview>"
		if item.TextPreview != nil {
			preview = *item.TextPreview
		}
		lines = append(lines, fmt.Sprintf("- [%s] %s  %s", item.ContentType, formatSearchTimestamp(item.ActiveTimeMs), preview))
		if detailed {
			lines = append(lines, "    entryId: "+item.EntryID, "    mimeType: "+item.MimeType)
			extensions := "<none>"
			if len(item.FileExtensions) > 0 {
				extensions = strings.Join(item.FileExtensions, ",")
			}
			lines = append(lines, "    extensions: "+extensions)
			if item.SourceDevice != nil {
				lines = append(lines, "    source: "+*item.SourceDevice)
			}
		}
	}
	return strings.Join(lines, "\n")
}

func renderSearchStatus(s searchStatus) string {
	reason := "none"
	if s.Reason != nil {
		reason = *s.Reason
	}
	started, completed := "never", "never"
	if s.LastRebuildStartedAtMs != nil {
		started = formatSearchTimestamp(*s.LastRebuildStartedAtMs)
	}
	if s.LastRebuildCompletedAtMs != nil {
		completed = formatSearchTimestamp(*s.LastRebuildCompletedAtMs)
	}
	return strings.Join([]string{
		"Search state: " + s.State,
		"Reason: " + reason,
		"Last rebuild started: " + started,
		"Last rebuild completed: " + completed,
	}, "\n")
}
