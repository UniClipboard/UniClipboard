package desktopui

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"image"
	"io"
	"mime"
	"path/filepath"
	"slices"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/BurntSushi/toml"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// Desktop uses the approved composition with service-owned data. Run View on
// the mygo thread; all completions are applied there, never by I/O workers.
type Desktop struct {
	d         *viewState
	quickHost *quickHost
}

func NewDesktop(ctx context.Context, client *api.Client) *Desktop {
	d := &viewState{page: "history", theme: "system", layout: "list", nav: uikit.NavState{Value: "all"}, settings: uikit.NavState{Value: "general"}, deviceList: uikit.SourceState{Value: "local"}, gridSize: .4, options: map[string]bool{}, choices: map[string]string{}}
	ctx, cancel := context.WithCancel(ctx)
	d.live = &liveState{ctx: ctx, cancel: cancel, client: client, jobs: make(chan func(context.Context) func(), 16), results: make(chan func(), 32), changed: make(chan struct{}, 1), items: map[string]api.HistoryItem{}, details: map[string]entry{}, pending: map[string]api.PendingPeer{}}
	go d.live.work()
	go d.live.follow()
	return &Desktop{d: d}
}
func (s *Desktop) View(c *ui.Context) { s.d.view(c) }
func (s *Desktop) Close() {
	if s.d.live != nil {
		s.d.live.cancel()
	}
}

// SetLoginStatus connects the existing launcher-owned native login receipt.
// The callback is read only on the view thread.
func (s *Desktop) SetLoginStatus(read func() (bool, string), retry func()) {
	s.d.live.loginStatus = read
	s.d.live.retryLogin = retry
}

type liveState struct {
	sendCancel                                               context.CancelFunc
	sendStage                                                string
	queryOverride                                            func() engine.HistorySearchQuery
	appliedSerial, contentSerial                             uint64
	searchCancel, catalogueCancel, countCancel, detailCancel context.CancelFunc
	detailGeneration                                         uint64
	loadGeneration                                           uint64
	catalogueKey, countKey                                   string
	catalogue                                                map[string][]engine.HistorySearchCandidate
	counts                                                   map[string]uint64

	ctx                                          context.Context
	cancel                                       context.CancelFunc
	client                                       *api.Client
	jobs                                         chan func(context.Context) func()
	results                                      chan func()
	changed                                      chan struct{}
	connected, loaded, loading, busy, detailBusy bool
	// loading serializes all history reads; foregroundLoading controls visible loading.
	foregroundLoading                          bool
	message, readError, cursor, key, detailKey string
	nextRefresh                                time.Time
	items                                      map[string]api.HistoryItem
	details                                    map[string]entry
	config                                     config.State
	pending                                    map[string]api.PendingPeer
	join                                       *api.PairResult
	codeAttempted, ticketAttempted             bool
	code                                       api.Code
	ticket                                     string
	loginStatus                                func() (bool, string)
	retryLogin                                 func()
}

func (l *liveState) work() {
	for {
		select {
		case <-l.ctx.Done():
			return
		case job := <-l.jobs:
			ctx, cancel := context.WithTimeout(l.ctx, 15*time.Second)
			apply := job(ctx)
			cancel()
			select {
			case <-l.ctx.Done():
				return
			case l.results <- apply:
			}
		}
	}
}
func (l *liveState) follow() {
	for l.ctx.Err() == nil {
		_ = l.client.Events(l.ctx, func(api.Event) {
			select {
			case l.changed <- struct{}{}:
			default:
			}
		})
		select {
		case l.changed <- struct{}{}:
		default:
		}
		select {
		case <-l.ctx.Done():
			return
		case <-time.After(time.Second):
		}
	}
}
func (l *liveState) enqueue(job func(context.Context) func()) bool {
	if l.ctx.Err() != nil {
		return false
	}
	select {
	case l.jobs <- job:
		return true
	default:
		l.message = "操作正在处理中，请稍后重试。"
		return false
	}
}
func (l *liveState) poll(d *viewState) {
	for {
		select {
		case f := <-l.results:
			f()
		default:
			goto drained
		}
	}
drained:
	dirty := false
	select {
	case <-l.changed:
		dirty = true
	default:
	}
	if dirty {
		l.nextRefresh = time.Time{}
	}
	l.loadCatalogue(d)
	key := d.liveQueryKey()
	if l.loading && key != l.key {
		l.invalidateSearch()
	}
	if !l.loading && !l.busy && (key != l.key || time.Now().After(l.nextRefresh)) {
		l.loadHistory(d, false)
	}
	if d.selected != "" && d.selected != l.detailKey {
		l.loadDetail(d)
	}
	// Pairing codes are created only while the invitation page is visible.
	if d.devicesOpen && d.deviceTask == "invite" && d.inviteMode != "ticket" && !l.busy && ((l.code.Code == "" && !l.codeAttempted) || (l.code.Code != "" && time.Now().UnixMilli() >= l.code.ExpiresAtMs)) {
		d.newLiveCode()
	}
	if d.devicesOpen && d.deviceTask == "invite" && d.inviteMode == "ticket" && !l.busy && l.ticket == "" && !l.ticketAttempted {
		d.newLiveTicket()
	}
}
func (d *viewState) liveQueryKey() string { return searchQueryKey(d.historySearchQuery()) }

func (d *viewState) liveQuery() api.HistoryQuery {
	q := api.HistoryQuery{Limit: 50, Tags: selectedSearchTags(d.tag)}
	switch d.kind {
	case "文本":
		q.Kinds = []string{"text"}
	case "图片":
		q.Kinds = []string{"image"}
	case "文件":
		q.Kinds = []string{"files"}
	case "文件夹":
		q.Kinds = []string{"directory", "directory_reference"}
	}
	if d.source == "local" {
		q.Origins = []string{"local_system", "explicit", "action"}
	} else if d.source != "all" {
		q.DeviceID = d.source
	}
	now := time.Now()
	start := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, now.Location())
	switch d.timeRange {
	case "today":
		q.FromMs = start.UnixMilli()
	case "yesterday":
		q.FromMs = start.AddDate(0, 0, -1).UnixMilli()
		q.BeforeMs = start.UnixMilli()
	case "day_before_yesterday":
		q.FromMs = start.AddDate(0, 0, -2).UnixMilli()
		q.BeforeMs = start.AddDate(0, 0, -1).UnixMilli()
	case "last_week":
		days := (int(start.Weekday()) + 6) % 7
		end := start.AddDate(0, 0, -days)
		q.FromMs = end.AddDate(0, 0, -7).UnixMilli()
		q.BeforeMs = end.UnixMilli()
	case "last_month":
		end := time.Date(now.Year(), now.Month(), 1, 0, 0, 0, 0, now.Location())
		q.FromMs = end.AddDate(0, -1, 0).UnixMilli()
		q.BeforeMs = end.UnixMilli()
	case "last_7d":
		q.FromMs = start.AddDate(0, 0, -6).UnixMilli()
	case "last_30d":
		q.FromMs = start.AddDate(0, 0, -29).UnixMilli()
	case "this_week":
		days := (int(start.Weekday()) + 6) % 7
		q.FromMs = start.AddDate(0, 0, -days).UnixMilli()
	case "this_month":
		q.FromMs = time.Date(now.Year(), now.Month(), 1, 0, 0, 0, 0, now.Location()).UnixMilli()
	}
	return q
}
func (d *viewState) unsupportedSearch() bool {
	return d.kind == "代码" || d.kind == "链接" || d.nav.Value == "pinned"
}
func (l *liveState) loadHistory(d *viewState, more bool) {
	if l.loading {
		return
	}
	q := engine.HistorySearchRequest{Query: d.historySearchQuery(), Limit: 50}
	target := max(50, len(d.entries))
	key := d.liveQueryKey()
	if key != l.key {
		target = 50
		d.entries = nil
		l.items = map[string]api.HistoryItem{}
		d.selected = ""
		l.detailKey = ""
		d.quickLook = false
	}
	if more {
		if l.cursor == "" || key != l.key {
			return
		}
		q.Cursor = l.cursor
	}
	l.foregroundLoading = !l.loaded || key != l.key
	l.loading = true
	l.loadGeneration++
	generation := l.loadGeneration
	ctx, cancel := context.WithTimeout(l.ctx, 10*time.Second)
	if l.searchCancel != nil {
		l.searchCancel()
	}
	l.searchCancel = cancel
	l.key = key
	l.nextRefresh = time.Now().Add(5 * time.Second)
	go func() {
		defer cancel()
		apply := func() func() {
			status, err := l.client.Status(ctx)
			var page api.HistorySearchPage
			var entries []entry
			var saved config.State
			var tags []string
			var candidates []api.Candidate
			if err == nil {
				page, err = l.client.SearchHistory(ctx, q)
				if !more && err == nil {
					for len(page.Hits) < target && page.NextCursor != "" {
						nextQuery := q
						nextQuery.Cursor = page.NextCursor
						next, e := l.client.SearchHistory(ctx, nextQuery)
						if e != nil {
							err = e
							break
						}
						page.Hits = append(page.Hits, next.Hits...)
						page.NextCursor = next.NextCursor
					}
				}
			}
			if err == nil {
				for _, hit := range page.Hits {
					v := projectHistory(hit.Item)
					if hit.Preview.Text != "" {
						v.title = shortText(hit.Preview.Text, 90)
					}
					v.highlights = hit.Preview.Highlights
					v.preview = hit.Preview.Text
					entries = append(entries, v)
				}
				saved, err = l.client.Config(ctx)
			}
			if err == nil {
				tags, err = l.client.HistoryTags(ctx)
			}
			if err == nil {
				candidates, err = l.client.Candidates(ctx)
			}
			return func() {
				if generation != l.loadGeneration {
					return
				}
				l.loading = false
				l.foregroundLoading = false
				if key != d.liveQueryKey() {
					l.nextRefresh = time.Time{}
					return
				}
				if err != nil {
					l.connected = status.ID != ""
					l.readError = "无法完成搜索。请稍后重试。"
					if api.ReasonOf(err) == "invalid_argument" {
						l.readError = "搜索条件无效或超过限制。"
					}
					l.detailKey = ""
					return
				}
				l.appliedSerial++
				l.connected = true
				l.loaded = true
				l.readError = ""
				if page.IndexIncomplete {
					l.readError = "部分内容的搜索索引不可用。元数据仍可浏览。"
				}
				if !more {
					d.entries = nil
					l.items = map[string]api.HistoryItem{}
				}
				l.countKey = ""
				l.counts = nil
				for i, hit := range page.Hits {
					it := hit.Item
					l.items[it.ID] = it
					if cached, ok := l.details[it.ID]; ok {
						entries[i].text = cached.text
						entries[i].image = cached.image
						entries[i].rich = cached.rich
						entries[i].contentError = cached.contentError
						entries[i].folder = cached.folder
					}
					at := slices.IndexFunc(d.entries, func(e entry) bool { return e.id == it.ID })
					if at < 0 {
						d.entries = append(d.entries, entries[i])
					} else {
						d.entries[at] = entries[i]
					}
				}
				// Keep detail memory bounded by the visible page set.
				for id := range l.details {
					if _, ok := l.items[id]; !ok {
						delete(l.details, id)
					}
				}
				l.cursor = page.NextCursor
				l.config = saved
				d.projectLiveConfig()
				d.projectTags(tags)
				previousName := ""
				if len(d.devices) > 0 {
					previousName = d.devices[0].name
				}
				d.devices = []device{{id: "local", name: status.Name, platform: "本机", icon: uikit.IconDesktop, online: true}}
				if d.deviceName == "" || d.deviceName == previousName {
					d.deviceName = status.Name
				}
				for _, p := range status.Peers {
					if p.Kind == "external" {
						continue
					}
					d.devices = append(d.devices, device{id: p.ID, name: p.Name, online: p.Connected, latency: -1, lastSeen: formatSeen(p.LastSeen), platform: "未提供", link: "在线", icon: uikit.IconDesktop})
				}
				d.pending = nil
				l.pending = map[string]api.PendingPeer{}
				for _, p := range status.Pending {
					l.pending[p.ID] = p
					d.pending = append(d.pending, pendingDevice{id: p.ID, name: p.Name, fingerprint: p.Fingerprint, via: p.Via, platform: "未提供", icon: uikit.IconDesktop})
				}
				d.candidates = nil
				for _, c := range candidates {
					if !c.Ignored {
						d.candidates = append(d.candidates, deviceCandidate{id: c.ID, name: c.Name, introducer: strings.Join(c.IntroducedBy, "、"), backendID: c.BackendID, icon: uikit.IconDesktop})
					}
				}
				if l.join != nil && slices.ContainsFunc(status.Peers, func(p api.PeerStatus) bool { return p.ID == l.join.ID }) {
					d.deviceTask = ""
					d.deviceList.Value = l.join.ID
					l.join = nil
				}
				if d.selected == "" && len(d.entries) > 0 {
					d.selectEntry(d.entries[0].id)
				}
				if !slices.ContainsFunc(d.entries, func(e entry) bool { return e.id == d.selected }) {
					d.selected = ""
					l.detailKey = ""
				}
			}
		}()
		select {
		case l.results <- apply:
		case <-l.ctx.Done():
		}
	}()

}
func projectHistory(it api.HistoryItem) entry {
	kind := uikit.KindText
	title := "文本"
	switch it.Kind {
	case "image":
		kind = uikit.KindImage
		title = "图片"
	case "directory", "directory_reference":
		kind = uikit.KindFolder
		title = "目录快照"
		if it.Kind == "directory_reference" {
			title = "源目录引用"
		}
	case "files":
		kind = uikit.KindFile
		title = "文件"
	}
	source := it.DeviceID
	if source == "" {
		source = "local"
	}
	if it.Origin == "external" || it.Origin == "migration" {
		source = it.Origin
	}
	return entry{generatedTitle: true, id: it.ID, title: title, source: source, date: formatDate(it.CreatedAt), size: fmt.Sprintf("%d B", it.TotalBytes), kind: kind, tags: slices.Clone(it.Tags), createdAt: it.CreatedAt, syncState: it.SyncState}
}
func formatDate(ms int64) string {
	t := time.UnixMilli(ms)
	now := time.Now()
	day := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, now.Location())
	if !t.Before(day) {
		return "今天 " + t.Format("15:04")
	}
	if !t.Before(day.AddDate(0, 0, -1)) {
		return "昨天 " + t.Format("15:04")
	}
	return t.Format("2006-01-02 15:04")
}
func formatSeen(ms int64) string {
	if ms == 0 {
		return "未提供"
	}
	return time.UnixMilli(ms).Format("2006-01-02 15:04")
}
func shortText(s string, n int) string {
	if utf8.RuneCountInString(s) > n {
		return string([]rune(s)[:n]) + "…"
	}
	return s
}
func (l *liveState) loadDetail(d *viewState) {
	id := d.selected
	it, ok := l.items[id]
	if !ok {
		return
	}
	l.detailBusy = true
	l.detailKey = id
	l.detailGeneration++
	generation := l.detailGeneration
	if l.detailCancel != nil {
		l.detailCancel()
	}
	ctx, cancel := context.WithTimeout(l.ctx, 8*time.Second)
	l.detailCancel = cancel
	go func() {
		defer cancel()

		v := loadSearchEntry(ctx, l.client, it)
		apply := func() {
			if generation != l.detailGeneration {
				return
			}
			l.detailBusy = false
			if d.selected != id {
				return
			}
			if _, ok := l.items[id]; !ok {
				return
			}
			l.contentSerial++
			// Keep only one full payload, regardless of how many pages are loaded.
			l.details = map[string]entry{id: v}
			for i := range d.entries {
				if d.entries[i].id != id {
					d.entries[i].text = ""
					d.entries[i].image = nil
					d.entries[i].rich = nil
					d.entries[i].contentError = ""
					d.entries[i].folder = nil
				}
			}
			for i := range d.entries {
				if d.entries[i].id == id {
					d.entries[i].text = v.text
					d.entries[i].image = v.image
					d.entries[i].rich = v.rich
					d.entries[i].contentError = v.contentError
					d.entries[i].folder = v.folder
				}
			}
		}
		select {
		case l.results <- apply:
		case <-l.ctx.Done():
		}
	}()

}

// mutate queues one user intent. Writes are never automatically replayed.
func (d *viewState) mutate(work func(context.Context) error, success string) {
	l := d.live
	if l.busy {
		return
	}
	selected := d.selected
	l.invalidateSearch()
	l.busy = true
	l.message = ""
	d.notice = "正在处理…"
	if !l.enqueue(func(ctx context.Context) func() {
		err := work(ctx)
		return func() {
			l.busy = false
			l.nextRefresh = time.Time{}
			if err != nil {
				l.message = "操作未能确认完成。请刷新数据后检查结果。"
				if api.ReasonOf(err) == "config_apply_failed" {
					l.message = "设置已保存，但未能生效。请检查设置后重试。"
				}
				if api.ReasonOf(err) == "conflict" {
					l.message = "数据已变化。请刷新后重试。"
				}
				d.notice = "操作失败"
			} else {
				l.message = ""
				d.notice = success
			}
			if d.selected != selected {
				d.notice = ""
			}
		}
	}) {
		l.busy = false
	}
}
func (d *viewState) useEntry() {
	if d.current() == nil {
		return
	}
	if d.live == nil {
		d.notice = "演示：已复制"
		return
	}
	id := d.selected
	request := "desktop-" + newRequestID()
	client := d.live.client
	d.mutate(func(ctx context.Context) error { _, err := client.RestoreHistory(ctx, id, request); return err }, "已复制")
}
func (d *viewState) setEntryTags(id string, tags []string) {
	if d.live == nil {
		for i := range d.entries {
			if d.entries[i].id == id {
				d.entries[i].tags = slices.Clone(tags)
			}
		}
		return
	}
	it, ok := d.live.items[id]
	if !ok {
		return
	}
	tags = slices.Clone(tags)
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		_, err := client.UpdateHistory(ctx, id, api.HistoryUpdate{Revision: it.Revision, Tags: &tags})
		return err
	}, "标签已保存")
}
func (d *viewState) deleteLiveEntries(ids []string) {
	if len(ids) == 0 {
		return
	}
	records := []api.HistoryItem{}
	for _, id := range ids {
		if it, ok := d.live.items[id]; ok {
			records = append(records, it)
		} else {
			d.live.message = "历史已变化。请刷新后重试。"
			return
		}
	}
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		for _, it := range records {
			if err := client.DeleteHistory(ctx, it.ID, it.Revision); err != nil {
				return err
			}
		}
		return nil
	}, "已删除")
}
func (d *viewState) saveLiveConfig(cfg config.Config) {
	revision := d.live.config.Revision
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		var buf bytes.Buffer
		if err := toml.NewEncoder(&buf).Encode(cfg); err != nil {
			return err
		}
		state, err := client.SaveConfig(ctx, revision, buf.Bytes())
		if err == nil && state.ApplyError != "" {
			return &api.Error{Message: "saved but not applied", Reason: "config_apply_failed"}
		}
		return err
	}, "设置已保存")
}
func (d *viewState) projectLiveConfig() {
	c := d.live.config.Config
	d.options["startup"] = c.Desktop.LaunchAtLogin
	d.options["sync"] = c.Sync.Enabled
	d.options["autoSync"] = c.Sync.Automatic
	d.options["restore"] = c.Sync.OnRestore
	d.options["telemetry"] = c.Telemetry.Enabled
}
func (d *viewState) newLiveCode() {
	if d.live.busy {
		return
	}
	d.live.codeAttempted = true
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		code, err := client.NewCode(ctx)
		if err == nil {
			select {
			case d.live.results <- func() { d.live.code = code; d.inviteCode = code.Code }:
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return err
	}, "配对码已生成")
}

func (d *viewState) newLiveTicket() {
	if d.live.busy {
		return
	}
	d.live.ticketAttempted = true
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		ticket, err := client.ExportTicket(ctx)
		if err == nil {
			select {
			case d.live.results <- func() { d.live.ticket = ticket }:
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return err
	}, "票据已生成")
}

func newRequestID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic(err)
	}
	return hex.EncodeToString(b[:])
}

func loadSearchEntry(ctx context.Context, client *api.Client, it api.HistoryItem) entry {
	id := it.ID
	v := projectHistory(it)
	switch it.Kind {
	case "directory", "directory_reference":
		var names []string
		info := &folderInfo{reference: it.Kind == "directory_reference"}
		v.folder = info
		for i, p := range it.Parts {
			if it.Kind == "directory" && p.MIME != engine.MimeDirectory {
				continue
			}
			if it.Kind == "directory_reference" && p.Kind != "file" {
				continue
			}
			r, err := client.HistoryContent(ctx, id, i)
			if err != nil {
				v.contentError = "目录内容不可用。"
				continue
			}
			if it.Kind == "directory_reference" {
				names = append(names, r.Name)
				_ = r.Close()
				continue
			}
			data, err := io.ReadAll(io.LimitReader(r, (512<<10)+1))
			_ = r.Close()
			var manifest engine.DirectoryManifest
			if err != nil || len(data) > 512<<10 || json.Unmarshal(data, &manifest) != nil {
				v.contentError = "目录内容不可用。"
				continue
			}
			for _, root := range manifest.Roots {
				names = append(names, root.Name)
			}
			const maxRows = 200
			for _, e := range manifest.Entries {
				if e.Path == "" {
					continue // a root itself
				}
				if e.Directory {
					info.dirs++
				} else {
					info.files++
					info.bytes += e.Size
				}
				if len(info.rows) >= maxRows {
					info.truncated = true
					continue
				}
				parts := strings.Split(e.Path, "/")
				info.rows = append(info.rows, folderRow{depth: len(parts) - 1, name: parts[len(parts)-1], dir: e.Directory, size: e.Size})
			}
		}
		info.names = names
		if info.reference {
			// Look at the live folder once per preview: direct children only.
			if l, err := client.HistoryFolder(ctx, id); err != nil {
				info.listError = true
			} else {
				for _, e := range l.Entries {
					if e.Directory {
						info.dirs++
					} else {
						info.files++
						info.bytes += e.Size
					}
					info.rows = append(info.rows, folderRow{name: e.Name, dir: e.Directory, size: e.Size})
				}
				info.truncated = l.Truncated
			}
		}
		if len(names) > 0 {
			v.title = shortText(strings.Join(names, ", "), 90)
		}
		v.text = strings.Join(names, "\n") + "\n目录快照。发送时使用已保存的内容。"
		if it.Kind == "directory_reference" {
			v.text = strings.Join(names, "\n") + "\n源目录引用。主动发送时读取当前目录内容。"
		}
		return v
	case "files":
		var names []string
		fileCount := 0
		for _, p := range it.Parts {
			if p.Kind == "file" {
				fileCount++
			}
		}
		for i, p := range it.Parts {
			if p.Kind != "file" {
				continue
			}
			r, e := client.HistoryContent(ctx, id, i)
			if e != nil {
				v.contentError = "文件内容不可用。"
				continue
			}
			names = append(names, r.Name)
			// Preview a single image file without changing its file/copy semantics.
			// Damaged or oversized images retain the ordinary file preview.
			imageFile := strings.HasPrefix(r.MIME, "image/") || strings.HasPrefix(mime.TypeByExtension(strings.ToLower(filepath.Ext(r.Name))), "image/")
			if fileCount == 1 && imageFile && p.Size <= 32<<20 && r.Size <= 32<<20 {
				v.image, _ = readPreviewImage(r)
			}
			_ = r.Close()
		}
		v.text = strings.Join(names, "\n")
		if len(names) > 0 {
			v.title = shortText(names[0], 90)
			v.generatedTitle = false
		}
	default:
		part := -1
		isHTML := false
		for i, p := range it.Parts {
			if (it.Kind == "image" && p.MIME == "image/png") || (it.Kind != "image" && p.MIME == "text/plain") {
				part = i
				break
			}
		}
		if part < 0 && it.Kind == "text" {
			for i, p := range it.Parts {
				if p.MIME == "text/html" {
					part = i
					isHTML = true
					break
				}
			}
		}
		if part < 0 {
			v.contentError = "此内容没有可显示的预览。"
			break
		}
		r, e := client.HistoryContent(ctx, id, part)
		if e != nil {
			v.contentError = "内容已不可用。"
			break
		}
		if it.Kind == "image" {
			v.image, v.contentError = readPreviewImage(r)
			_ = r.Close()
			break
		}
		limit := int64(1 << 20)
		raw, e := io.ReadAll(io.LimitReader(r, limit+1))
		_ = r.Close()
		if e != nil {
			v.contentError = "读取内容失败。"
			break
		}
		if int64(len(raw)) > limit {
			v.contentError = "内容超过预览大小限制。"
			break
		}
		v.text = strings.ToValidUTF8(string(raw), "�")
		if isHTML {
			parsed := richtext.ParseHTML(v.text)
			v.rich = richtext.NewDocument(parsed)
			v.text = parsed.PlainText()
		}
		v.title = shortText(strings.TrimSpace(v.text), 90)
		v.generatedTitle = false
	}
	return v
}

// readPreviewImage bounds encoded bytes and decoded pixels before allocating a bitmap.
func readPreviewImage(r io.Reader) (ui.ImageSource, string) {
	const limit = 32 << 20
	raw, err := io.ReadAll(io.LimitReader(r, limit+1))
	if err != nil {
		return nil, "读取内容失败。"
	}
	if len(raw) > limit {
		return nil, "内容超过预览大小限制。"
	}
	cfg, _, err := image.DecodeConfig(bytes.NewReader(raw))
	if err != nil || cfg.Width <= 0 || cfg.Height <= 0 || int64(cfg.Width) > 16_000_000/int64(cfg.Height) {
		return nil, "图片无法预览。"
	}
	src, err := ui.DecodeImage(raw)
	if err != nil {
		return nil, "图片无法预览。"
	}
	return src, ""
}
