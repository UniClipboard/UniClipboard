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
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/BurntSushi/toml"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/sourceapp"
)

// Desktop uses the approved composition with service-owned data. Run View on
// the mygo thread; all completions are applied there, never by I/O workers.
type Desktop struct {
	d *viewState
}

func NewDesktop(ctx context.Context, client *api.Client) *Desktop {
	d := &viewState{page: "history", theme: "system", layout: "list", nav: desktopkit.NavState{Value: "all"}, settings: desktopkit.NavState{Value: "general"}, deviceList: desktopkit.SourceState{Value: "local"}, gridSize: .4, options: map[string]bool{}, choices: map[string]string{}}
	ctx, cancel := context.WithCancel(ctx)
	d.live = &liveState{ctx: ctx, cancel: cancel, client: client, jobs: make(chan func(context.Context) func(), 16), results: make(chan func(), 32), changed: make(chan struct{}, 1), opsChanged: make(chan struct{}, 1), opsDirty: true, recovery: make(chan struct{}, 1), searchSlots: make(chan struct{}, 1), items: map[string]api.HistoryItem{}, details: map[string]entry{}, pending: map[string]api.PendingPeer{}, opsByHistory: map[string]engine.Operation{}, discardedReceives: map[string]struct{}{}}
	context.AfterFunc(ctx, d.live.stopWake)
	go d.live.work()
	go d.live.follow()
	return &Desktop{d: d}
}
func (s *Desktop) View(c *ui.Context) { s.d.view(c) }

// SetWindowToolbar binds host controls and the frame value used by this GUI.
// Call on the view thread before the first frame. A nil slot uses native controls.
func (s *Desktop) SetWindowToolbar(systemFrame bool, trailing func(*ui.Context)) {
	s.d.windowFrame = &systemFrame
	s.d.toolbarTrailing = trailing
}

func (d *viewState) trailingToolbar(c *ui.Context) func() {
	if d.toolbarTrailing == nil {
		return nil
	}
	return func() { d.toolbarTrailing(c) }
}

func (s *Desktop) Close() {
	if s.d.live != nil {
		s.d.live.cancel()
		if closeClipboard := s.d.live.closeClipboardFallback; closeClipboard != nil {
			closeClipboard()
		}
	}
}

// SetLoginStatus connects the existing launcher-owned native login receipt.
// The callback is read only on the view thread.
func (s *Desktop) SetLoginStatus(read func() (bool, string), retry func()) {
	s.d.live.loginStatus = read
	s.d.live.retryLogin = retry
}

// SetClipboardFallback connects the explicit desktop copy path on sessions
// that cannot enable a background clipboard adapter.
func (s *Desktop) SetClipboardFallback(unavailable func() bool, copy func(context.Context, string) error, close func()) {
	s.d.live.clipboardUnavailable = unavailable
	s.d.live.copyFallback = copy
	s.d.live.closeClipboardFallback = close
}

// SetMigrationOpener delegates exclusive runtime ownership to the Desktop host.
func (s *Desktop) SetMigrationOpener(open func()) { s.d.live.openMigration = open }

// SetRestarter delegates GUI recreation or a restart that includes the
// tray-owned service. The host reports failures to a still-open GUI.
func (s *Desktop) SetRestarter(restart func(restartService bool, done func(error))) {
	s.d.live.restartApp = restart
}

type liveState struct {
	clipboardUnavailable   func() bool
	copyFallback           func(context.Context, string) error
	closeClipboardFallback func()

	wakeMu       sync.Mutex
	wakeServices ui.Services
	wakeTimer    *time.Timer
	wakeDue      time.Time

	openMigration                              func()
	restartApp                                 func(restartService bool, done func(error))
	restarting                                 bool
	sendCancel                                 context.CancelFunc
	sendStage                                  string
	queryOverride                              func() engine.HistorySearchQuery
	appliedSerial, contentSerial               uint64
	resultEpoch                                uint64
	searchCancel, catalogueCancel, countCancel context.CancelFunc
	sidebarCountCancel                         context.CancelFunc
	detailEpoch                                uint64
	detailWindow                               []string
	detailReads                                map[string]*detailRead
	loadGeneration                             uint64
	catalogueKey, countKey, sidebarCountKey    string
	catalogue                                  map[string][]engine.HistorySearchCandidate
	counts                                     map[string]uint64
	sidebarCounts                              map[string]uint64

	ctx                                          context.Context
	cancel                                       context.CancelFunc
	actionCancel                                 context.CancelFunc
	pendingSelectID                              string
	client                                       *api.Client
	jobs                                         chan func(context.Context) func()
	results                                      chan func()
	changed                                      chan struct{}
	opsChanged                                   chan struct{}
	opsLoading, opsDirty                         bool
	opsNextRefresh                               time.Time
	activeOps                                    []engine.Operation
	activeReceives                               []engine.Operation
	discardedReceives                            map[string]struct{}
	opsByHistory                                 map[string]engine.Operation
	recovery                                     chan struct{}
	searchSlots                                  chan struct{}
	connected, loaded, loading, busy, detailBusy bool
	// loading serializes all history reads; foregroundLoading controls visible loading.
	foregroundLoading                                                                        bool
	loadingSince, detailLoadingSince, busySince                                              time.Time
	busyNotice                                                                               string
	message, readError, cursor, key, displayedKey, displayedScope, lastResultKind, detailKey string
	nextRefresh                                                                              time.Time
	pageRetry                                                                                time.Time
	historyRevision                                                                          uint64
	historyInvalid                                                                           bool
	serviceID                                                                                string
	windowOffset                                                                             int
	previousPage                                                                             bool
	checkpoints                                                                              []historyCheckpoint
	retained                                                                                 *entry
	items                                                                                    map[string]api.HistoryItem
	details                                                                                  map[string]entry
	config                                                                                   config.State
	actions                                                                                  []api.Action
	syncBackends                                                                             []api.SyncBackendInfo
	pairingBackend                                                                           string
	externalPlugins                                                                          []api.PluginInfo
	marketplace                                                                              api.PluginMarketplace
	marketplaceLoaded                                                                        bool
	marketplaceLoading                                                                       bool
	marketplaceError                                                                         string
	peers                                                                                    map[string]api.PeerStatus
	pending                                                                                  map[string]api.PendingPeer
	taskKnownPending                                                                         map[string]int64
	join                                                                                     *api.PairResult
	codeAttempted, ticketAttempted                                                           bool
	code                                                                                     api.Code
	ticket                                                                                   string
	loginStatus                                                                              func() (bool, string)
	retryLogin                                                                               func()
}

func (l *liveState) work() {
	for {
		select {
		case <-l.ctx.Done():
			return
		case job := <-l.jobs:
			apply := job(l.ctx)
			select {
			case <-l.ctx.Done():
				return
			case l.results <- apply:
				l.wake()
			}
		}
	}
}
func (l *liveState) follow() {
	for l.ctx.Err() == nil {
		_ = l.client.Events(l.ctx, func(ev api.Event) {
			switch ev.Kind {
			case "operations":
				l.markOperationsDirty()
				return
			case "history", "sent":
				l.markOperationsDirty()
			case "config", "peer-up", "peer-down", "peers", "pending", "code", "candidates", "channel", "received":
			default:
				return
			}
			select {
			case l.changed <- struct{}{}:
				l.wake()
			default:
			}
		})
		select {
		case l.recovery <- struct{}{}:
			l.wake()
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
	return l.enqueueWithin(job, 15*time.Second)
}
func (l *liveState) enqueueWithin(job func(context.Context) func(), budget time.Duration) bool {
	wrapped := func(parent context.Context) func() {
		ctx, cancel := context.WithTimeout(parent, budget)
		defer cancel()
		return job(ctx)
	}
	if l.ctx.Err() != nil {
		return false
	}
	select {
	case l.jobs <- wrapped:
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
	select {
	case <-l.opsChanged:
		l.opsDirty = true
	default:
	}
	select {
	case <-l.recovery:
		l.historyInvalid = true
		l.invalidateSearch()
		l.opsDirty = true
		dirty = true
	default:
	}
	if dirty {
		l.nextRefresh = time.Time{}
	}
	l.loadOperations(d)
	l.loadCatalogue(d)
	l.loadSidebarCounts(d)
	key := d.liveQueryKey()
	if l.loading && key != l.key {
		l.invalidateSearch()
	}
	if key != l.key {
		l.startForegroundLoading()
	}
	if !l.loading && !l.busy && (key != l.key || (time.Now().After(l.nextRefresh) && !time.Now().Before(l.pageRetry))) {
		l.loadHistory(d, false)
	}
	l.loadDetail(d)
	// Pairing codes are created only while the invitation page is visible and no inline approval has taken over.
	if d.page == "devices" && d.deviceTask == "invite" && d.matchingInlinePending() == nil && l.pairingBackend != "" {
		if d.inviteMode != "ticket" && !l.busy && ((l.code.Code == "" && !l.codeAttempted) || (l.code.Code != "" && time.Now().UnixMilli() >= l.code.ExpiresAtMs)) {
			d.newLiveCode()
		}
		if d.inviteMode == "ticket" && !l.busy && l.ticket == "" && !l.ticketAttempted {
			d.newLiveTicket()
		}
	}
	l.scheduleWake(d)
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
	return d.kind == "代码" || d.kind == "链接"
}
func (l *liveState) loadHistory(d *viewState, more bool) {
	if l.loading {
		return
	}
	if l.searchSlots == nil {
		l.searchSlots = make(chan struct{}, 1)
	}
	select {
	case l.searchSlots <- struct{}{}:
	default:
		return
	}
	started := false
	defer func() {
		if !started {
			<-l.searchSlots
		}
	}()
	q := engine.HistorySearchRequest{Query: d.historySearchQuery(), Limit: 50}
	favoritesOnly := slices.Contains(q.Query.Tags, plugin.TagFavorite) && q.Query.TagMatch != "any"
	previous := l.previousPage
	l.previousPage = false
	target := min(historyWindowLimit, max(50, len(d.entries)))
	offset := l.windowOffset
	points := slices.Clone(l.checkpoints)
	// These are stable IDs from the last rendered viewport. A binding with
	// no rendered viewport has no scroll anchor to preserve.
	anchors := slices.Clone(l.detailWindow[:min(50, len(l.detailWindow))])
	if len(anchors) == 0 && offset > 0 {
		for _, v := range d.entries[:min(50, len(d.entries))] {
			anchors = append(anchors, v.id)
		}
	}
	selected := d.selected
	prevSelected := d.selected
	selectedItem := l.items[selected]
	prevSelectedItem := selectedItem
	var selectedEntry *entry
	if v := d.current(); v != nil {
		copy := *v
		selectedEntry = &copy
	}
	revision, serviceID := l.historyRevision, l.serviceID
	invalid := l.historyInvalid
	selectedRevision := selectedItem.Revision
	key := d.liveQueryKey()
	scope := d.liveScopeKey()
	wasLoaded := l.loaded
	queryChanged := !l.loaded || key != l.displayedKey
	foreground := queryChanged
	expired := false
	for _, it := range l.items {
		if it.ExpiresAt > 0 && it.ExpiresAt <= time.Now().UnixMilli() {
			expired = true
			break
		}
	}
	if queryChanged {
		previous = false
		target = 50
		offset = 0
		points = nil
		anchors = nil
		selected = ""
		selectedEntry = nil
	}
	if more {
		if l.cursor == "" || key != l.key || queryChanged {
			return
		}
		q.Cursor = l.cursor
		points = rememberHistoryCheckpoint(points, historyCheckpoint{offset + len(d.entries), q.Cursor})
	}
	if foreground {
		l.startForegroundLoading()
	} else {
		l.stopForegroundLoading()
	}
	l.loading = true
	l.loadGeneration++
	generation := l.loadGeneration
	ctx, cancel := context.WithTimeout(l.ctx, 10*time.Second)
	if l.searchCancel != nil {
		l.searchCancel()
	}
	l.searchCancel = cancel
	l.key = key
	l.nextRefresh = time.Now().Add(historyRecoveryInterval)
	firstPanelPage := l.queryOverride != nil && !wasLoaded && !more && !previous
	started = true
	go func() {
		defer func() {
			<-l.searchSlots
			if firstPanelPage {
				l.wake()
			}
		}()
		defer cancel()
		publish := func(apply func()) bool {
			select {
			case l.results <- apply:
				l.wake()
				return true
			case <-l.ctx.Done():
				return false
			}
		}
		apply := func() func() {
			var status api.Status
			var err error
			if !firstPanelPage {
				status, err = l.client.Status(ctx)
			}
			var page api.HistorySearchPage
			var entries []entry
			var saved config.State
			var actions []api.Action
			var externalPlugins []api.PluginInfo
			var syncBackends []api.SyncBackendInfo
			var tags []string
			var candidates []api.Candidate
			historyChanged := more || previous
			// Query state was captured on the view thread.
			historyChanged = historyChanged || invalid
			if err == nil && !more && !previous && !firstPanelPage {
				probe, e := l.client.QueryHistory(ctx, api.HistoryQuery{Limit: 1})
				err = e
				historyChanged = historyChanged || probe.Revision != revision || status.ID != serviceID || expired
			}
			// A new query always has a foreground load, regardless of revision.
			historyChanged = historyChanged || foreground
			if err == nil && historyChanged {
				page, offset, points, err = readHistoryWindow(ctx, l.client, q, more, previous, offset, target, anchors, points, revision)
				if err == nil && selectedEntry != nil && !more {
					current, e := l.client.GetHistory(ctx, selected)
					if e != nil && api.ReasonOf(e) != "not_found" {
						err = e
					}
					if e != nil || favoritesOnly && !slices.Contains(current.Tags, plugin.TagFavorite) {
						selectedEntry = nil
					} else {
						selectedItem = current
						if current.Revision != selectedRevision {
							v := projectHistory(current)
							if !historyContentChanged(prevSelectedItem, current) {
								v.title = selectedEntry.title
								v.preview = selectedEntry.preview
								v.highlights = selectedEntry.highlights
							}
							selectedEntry = &v
						}
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
			}
			applyPage := func(err error, statusID string) bool {
				if generation != l.loadGeneration {
					return false
				}
				l.loading = false
				l.stopForegroundLoading()
				if key != d.liveQueryKey() {
					l.nextRefresh = time.Time{}
					return false
				}
				if err != nil {
					l.connected = statusID != ""
					l.readError = "无法完成搜索。请稍后重试。"
					if api.ReasonOf(err) == "invalid_argument" {
						l.readError = "搜索条件无效或超过限制。"
					}
					l.pageRetry = time.Now().Add(2 * time.Second)
					l.nextRefresh = l.pageRetry
					if more || previous {
						l.historyInvalid = true
					}
					return false
				}
				l.serviceID = statusID
				l.readError = ""
				if historyChanged {
					l.appliedSerial++
					l.connected = true
					l.loaded = true
					l.readError = ""
					if page.IndexIncomplete {
						l.readError = "部分内容的搜索索引不可用。元数据仍可浏览。"
					}
					if queryChanged {
						l.detailEpoch++
						l.detailWindow = nil
						for _, read := range l.detailReads {
							read.obsolete = true
							read.cancel()
						}
						l.detailKey, l.detailBusy = "", false
						l.retained = nil
						d.quickLook = false
					}
					if !more {
						fresh := make(map[string]api.HistoryItem, len(page.Hits))
						for _, hit := range page.Hits {
							fresh[hit.Item.ID] = hit.Item
						}
						for id, old := range l.items {
							if it, ok := fresh[id]; (!ok && id != selected) || (ok && (historyContentChanged(old, it) || (old.Revision != it.Revision && l.details[id].contentError != ""))) {
								l.dropDetail(d, id)
							}
						}
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
							copyDetail(&entries[i], cached)
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
						if _, ok := l.items[id]; !ok && id != selected {
							delete(l.details, id)
						}
					}
					if selectedEntry != nil {
						if historyContentChanged(prevSelectedItem, selectedItem) || (selectedItem.Revision != selectedRevision && l.details[selected].contentError != "") {
							l.dropDetail(d, selected)
						} else if cached, ok := l.details[selected]; ok {
							copyDetail(selectedEntry, cached)
						}
						l.retained = selectedEntry
						l.items[selected] = selectedItem
					} else {
						l.retained = nil
					}
					if more && len(d.entries) > historyWindowLimit {
						drop := len(d.entries) - historyWindowLimit
						d.entries = slices.Clone(d.entries[drop:])
						offset += drop
					}
					l.windowOffset = offset
					l.checkpoints = points
					l.historyRevision = page.HistoryRevision
					l.historyInvalid = false
					l.pageRetry = time.Time{}
					keep := map[string]bool{selected: selectedEntry != nil}
					for _, v := range d.entries {
						keep[v.id] = true
					}
					for id := range l.items {
						if !keep[id] {
							delete(l.items, id)
							l.dropDetail(d, id)
						}
					}
					l.cursor = page.NextCursor
					selectedPending := false
					if l.pendingSelectID != "" {
						if slices.ContainsFunc(d.entries, func(e entry) bool { return e.id == l.pendingSelectID }) {
							d.selectEntry(l.pendingSelectID)
							d.detailOpen, d.inspector = true, true
							d.list.ScrollTo(l.pendingSelectID)
							selectedPending = true
						}
						l.pendingSelectID = ""
					}
					if queryChanged {
						sameScope := wasLoaded && l.displayedScope == scope
						if !wasLoaded {
							l.lastResultKind = "cold"
						} else if !sameScope {
							l.lastResultKind = "scope"
						} else {
							l.lastResultKind = "refine"
						}
						l.displayedKey = key
						l.displayedScope = scope
						l.resultEpoch++
						if !selectedPending {
							if len(d.entries) > 0 {
								if sameScope && prevSelected != "" && slices.ContainsFunc(d.entries, func(e entry) bool { return e.id == prevSelected }) {
									d.selectEntry(prevSelected)
								} else {
									d.selectEntry(d.entries[0].id)
									d.list.ScrollTo(d.entries[0].id)
								}
							} else {
								d.selectEntry("")
								l.detailKey = ""
								l.detailLoadingSince = time.Time{}
								d.quickLook, d.detailOpen, d.inspector = false, false, false
							}
						}
					}
				}
				return true
			}
			if firstPanelPage {
				// The panel can show and act on real rows before settings, tags
				// and device metadata arrive. Keep the page error independent.
				pageErr := err
				if !publish(func() { applyPage(pageErr, "") }) || pageErr != nil {
					return nil
				}
				status, err = l.client.Status(ctx)
			}
			if err == nil {
				saved, err = l.client.Config(ctx)
				actions, _ = l.client.Actions(ctx)
				externalPlugins, _ = l.client.Plugins(ctx)
				syncBackends, _ = l.client.SyncBackends(ctx)
			}
			if err == nil {
				tags, err = l.client.HistoryTags(ctx)
			}
			if err == nil {
				candidates, err = l.client.Candidates(ctx)
			}
			return func() {
				if firstPanelPage {
					if err != nil || generation != l.loadGeneration || key != d.liveQueryKey() || key != l.displayedKey {
						return
					}
					l.appliedSerial++
					l.serviceID = status.ID
				} else if !applyPage(err, status.ID) {
					return
				}
				l.connected = true
				l.config = saved
				l.actions = actions
				l.externalPlugins = externalPlugins
				l.syncBackends = syncBackends
				d.projectLiveSyncChannels()
				d.projectLiveConfig()
				d.projectTags(tags)
				previousName := ""
				if len(d.devices) > 0 {
					previousName = d.devices[0].name
				}
				d.devices = []device{{id: "local", name: status.Name, deviceID: status.ID, icon: desktopkit.IconDesktop, online: true}}
				if d.deviceName == "" || d.deviceName == previousName {
					d.deviceName = status.Name
				}
				implByID := map[string]string{}
				for _, b := range syncBackends {
					implByID[b.ID] = b.ImplementationID
				}
				for _, pl := range externalPlugins {
					if _, ok := implByID[pl.ID]; !ok {
						implByID[pl.ID] = pl.ImplementationID
					}
				}
				l.peers = make(map[string]api.PeerStatus, len(status.Peers))
				d.channels = nil
				for _, p := range status.Peers {
					l.peers[p.ID] = p
					if p.Kind == "external" {
						d.channels = append(d.channels, externalChannel{
							id:               p.ID,
							name:             p.Name,
							backendID:        p.BackendID,
							enabled:          p.Enabled,
							autoApply:        p.AutoApply,
							remoteRevocation: p.RemoteRevocation,
							revision:         p.Revision,
							lastSeen:         formatSeen(p.LastSeen),
							lastError:        p.LastError,
						})
						continue
					}
					bindings := make([]deviceBinding, 0, len(p.Bindings))
					for _, b := range p.Bindings {
						impl := implByID[b.InstanceID]
						if impl == "" {
							impl = b.InstanceID
						}
						bindings = append(bindings, deviceBinding{
							id:               b.ID,
							instanceID:       b.InstanceID,
							implementationID: impl,
							state:            b.State,
							syncEnabled:      b.SyncEnabled,
							policy:           b.Policy.Normalized(),
							connection:       string(b.Connection),
							connectionPath:   b.ConnectionPath,
							latencyMs:        b.LatencyMs,
							lastChecked:      formatLastActiveMs(b.LastCheckedMs),
							revision:         b.Revision,
							lastActive:       formatLastActiveMs(b.LastActiveMs),
							lastError:        b.LastError,
							relayRouteReady:  b.RelayRouteReady,
						})
					}
					d.devices = append(d.devices, device{
						id:           p.ID,
						name:         p.Name,
						userName:     p.UserName,
						reportedName: p.ReportedName,
						icon:         desktopkit.IconDesktop,
						online:       p.Connected,
						lastSeen:     formatSeen(p.LastSeen),
						fingerprint:  p.Fingerprint,
						revision:     p.Revision,
						bindings:     bindings,
					})
				}
				d.pending = nil
				l.pending = map[string]api.PendingPeer{}
				for _, p := range status.Pending {
					l.pending[p.ID] = p
					paths := make([]pendingPath, 0, len(p.Paths))
					for _, path := range p.Paths {
						if path.State == "pending" {
							paths = append(paths, pendingPath{id: path.ID, instanceID: path.InstanceID, state: path.State, revision: path.Revision})
						}
					}
					d.pending = append(d.pending, pendingDevice{
						id:          p.ID,
						name:        p.Name,
						via:         p.Via,
						byCode:      p.ByCode,
						fingerprint: p.Fingerprint,
						revision:    p.Revision,
						icon:        desktopkit.IconDesktop,
						paths:       paths,
					})
				}
				d.candidates = nil
				for _, c := range candidates {
					if !c.Ignored {
						d.candidates = append(d.candidates, deviceCandidate{id: c.ID, name: c.Name, introducer: strings.Join(c.IntroducedBy, "、"), backendID: c.BackendID, icon: desktopkit.IconDesktop})
					}
				}
				if l.join != nil {
					inPeers := slices.ContainsFunc(status.Peers, func(p api.PeerStatus) bool { return p.ID == l.join.ID })
					inPending := slices.ContainsFunc(status.Pending, func(p api.PendingPeer) bool { return p.ID == l.join.ID })
					if inPeers && !inPending && (d.deviceTaskForID == "" || d.deviceHasAuthorizedBackend(l.join.ID, l.pairingBackend)) {
						d.deviceTask = ""
						d.deviceTaskForID = ""
						d.deviceList.Value = l.join.ID
						l.join = nil
					}
				}
				if d.selected == "" && len(d.entries) > 0 {
					d.selectEntry(d.entries[0].id)
				}
				if d.current() == nil {
					d.selectEntry("")
					l.detailKey = ""
					d.quickLook, d.detailOpen, d.inspector = false, false, false
				}
			}
		}()
		if apply != nil {
			publish(apply)
		}
	}()

}
func projectHistory(it api.HistoryItem) entry {
	kind := desktopkit.KindText
	title := "文本"
	switch it.Kind {
	case "image":
		kind = desktopkit.KindImage
		title = "图片"
	case "directory", "directory_reference":
		kind = desktopkit.KindFolder
		title = "目录快照"
		if it.Kind == "directory_reference" {
			title = "源目录引用"
		}
	case "files":
		kind = desktopkit.KindFile
		title = "文件"
	}
	source := it.DeviceID
	if source == "" {
		source = "local"
	}
	if it.Origin == "external" || it.Origin == "migration" {
		source = it.Origin
	}
	v := entry{generatedTitle: true, id: it.ID, title: title, source: source, date: formatDate(it.CreatedAt), size: humanBytes(it.TotalBytes), kind: kind, tags: withFavoriteTag(it.Tags, false), pinned: slices.Contains(it.Tags, plugin.TagFavorite), createdAt: it.CreatedAt, syncState: it.SyncState, backendSync: slices.Clone(it.Backends)}
	if app := it.SourceApplication; app != nil {
		v.sourceAppName, v.sourceAppID = strings.TrimSpace(app.Name), app.Identifier
		if v.sourceAppName == "" {
			v.sourceAppName = app.Identifier
		}
		if len(app.IconPNG) > 0 && sourceapp.ValidIcon(app.IconPNG) {
			v.sourceAppIcon, _ = ui.DecodeImage(app.IconPNG)
		}
	}
	return v
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
func formatSeen(sec int64) string {
	if sec <= 0 {
		return ""
	}
	return formatRelativeAt(time.Unix(sec, 0), time.Now())
}

func formatLastActiveMs(ms int64) string {
	if ms <= 0 {
		return ""
	}
	return formatRelativeAt(time.UnixMilli(ms), time.Now())
}

func formatRelativeAt(t, now time.Time) string {
	t = t.In(now.Location())
	diff := now.Sub(t)
	if diff < time.Minute && diff > -time.Minute {
		return "刚刚"
	}
	if diff >= time.Minute && diff < time.Hour {
		return fmt.Sprintf("%d 分钟前", int(diff/time.Minute))
	}
	day := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, now.Location())
	if !t.Before(day) && !t.After(now.Add(time.Minute)) {
		return "今天 " + t.Format("15:04")
	}
	if !t.Before(day.AddDate(0, 0, -1)) && t.Before(day) {
		return "昨天 " + t.Format("15:04")
	}
	return t.Format("2006-01-02 15:04")
}

func liveErrorMessage(err error) string {
	switch api.ReasonOf(err) {
	case "config_apply_failed":
		return "设置已保存，但未能生效。请检查设置后重试。"
	case "action_apply_failed":
		return "动作结果已保存到历史记录。未能复制到剪贴板。"
	case "conflict":
		return "数据已变化。请刷新后重试。"
	case "invalid":
		return "配对码或票据格式无效，请检查后重试。"
	case "incomplete":
		return "请输入完整的 6 位配对码或完整票据。"
	case "expired":
		return "配对码或票据已过期，请让对方刷新后重试。"
	case "rate-limited":
		return "尝试次数过多，请稍后再试。"
	case "unsupported":
		return "当前选择的本地同步渠道与该票据不兼容，请切换为对应的同步渠道。"
	case "unavailable":
		return "对应同步渠道当前不可用，请检查插件运行状态。"
	case "identity_mismatch":
		return "设备身份校验不匹配，请核对指纹或重新配对。"
	case "timeout":
		return "连接超时，请确认双方网络可达后重试。"
	}
	return "操作未能确认完成。请刷新数据后检查结果。"
}

func (d *viewState) projectLiveSyncChannels() {
	l := d.live
	cfg := l.config.Config
	pluginByID := map[string]api.PluginInfo{}
	for _, pl := range l.externalPlugins {
		pluginByID[pl.ID] = pl
	}
	seen := map[string]bool{}
	channels := make([]syncChannel, 0, len(l.syncBackends)+len(l.externalPlugins)+1)
	boolOff := func(v *bool) bool { return v != nil && !*v }
	for _, b := range l.syncBackends {
		seen[b.ID] = true
		restart := false
		if pl, ok := pluginByID[b.ID]; ok {
			restart = pl.Restart
		} else if b.ID == "builtin.tailcat" && !cfg.PluginEnabled("builtin.tailcat") {
			restart = true
		}
		sc := cfg.Plugins.Instances[b.ID].Sync
		channels = append(channels, syncChannel{
			id:               b.ID,
			implementationID: b.ImplementationID,
			transportKind:    b.TransportKind,
			state:            b.State,
			pairing:          b.Pairing,
			externalChannels: b.ExternalChannels,
			restart:          restart,
			policy:           syncPolicyLabel(sc),
			noText:           boolOff(sc.Text),
			noImages:         boolOff(sc.Images),
			noFiles:          boolOff(sc.Files),
		})
	}
	if !seen["builtin.tailcat"] {
		seen["builtin.tailcat"] = true
		sc := cfg.Plugins.Instances["builtin.tailcat"].Sync
		channels = append(channels, syncChannel{
			id:               "builtin.tailcat",
			implementationID: "builtin.tailcat",
			transportKind:    "peer_sessions",
			state:            "disabled",
			pairing:          true,
			restart:          cfg.PluginEnabled("builtin.tailcat"),
			policy:           syncPolicyLabel(sc),
			noText:           boolOff(sc.Text),
			noImages:         boolOff(sc.Images),
			noFiles:          boolOff(sc.Files),
		})
	}
	for _, pl := range l.externalPlugins {
		if seen[pl.ID] || !slices.Contains(pl.Capabilities, "sync_backend") {
			continue
		}
		seen[pl.ID] = true
		sc := cfg.Plugins.Instances[pl.ID].Sync
		channels = append(channels, syncChannel{
			id:               pl.ID,
			implementationID: pl.ImplementationID,
			transportKind:    pl.TransportKind,
			state:            "disabled",
			pairing:          pl.TransportKind == "peer_sessions" && !pl.ExternalChannels,
			externalChannels: pl.ExternalChannels,
			restart:          pl.Restart || pl.Configured,
			policy:           syncPolicyLabel(sc),
			noText:           boolOff(sc.Text),
			noImages:         boolOff(sc.Images),
			noFiles:          boolOff(sc.Files),
		})
	}
	d.syncChannels = channels
	restartPending := false
	for _, key := range l.config.RestartRequired {
		if key == "plugins" || strings.HasPrefix(key, "plugins.") {
			restartPending = true
			break
		}
	}
	if !restartPending {
		for _, ch := range channels {
			if ch.restart {
				restartPending = true
				break
			}
		}
	}
	d.restartPending = restartPending

	// Keep l.pairingBackend pinned to a currently running pairing channel.
	stillValid := false
	for _, b := range l.syncBackends {
		if b.ID == l.pairingBackend && b.Pairing && b.State == "running" {
			stillValid = true
			break
		}
	}
	if !stillValid {
		l.pairingBackend = ""
		for _, b := range l.syncBackends {
			if b.Pairing && b.State == "running" {
				l.pairingBackend = b.ID
				if b.ID == "builtin.tailcat" {
					break
				}
			}
		}
	}
}

func syncPolicyLabel(syncCfg config.BackendSync) string {
	on := func(v *bool) bool { return v == nil || *v }
	var parts []string
	if on(syncCfg.Text) {
		parts = append(parts, "文本")
	}
	if on(syncCfg.Images) {
		parts = append(parts, "图片")
	}
	if on(syncCfg.Files) {
		if syncCfg.AutoFileMaxBytes > 0 {
			parts = append(parts, fmt.Sprintf("文件 ≤%d MB", int64(syncCfg.AutoFileMaxBytes)/(1<<20)))
		} else {
			parts = append(parts, "文件")
		}
	}
	return strings.Join(parts, " · ")
}

func (d *viewState) deviceHasAuthorizedBackend(deviceID, backendID string) bool {
	if backendID == "" {
		return true
	}
	dev := d.deviceByID(deviceID)
	if dev == nil {
		return false
	}
	for _, b := range dev.bindings {
		if b.state == "authorized" && b.instanceID == backendID {
			return true
		}
	}
	return false
}
func shortText(s string, n int) string {
	if utf8.RuneCountInString(s) > n {
		return string([]rune(s)[:n]) + "…"
	}
	return s
}

func historyContentChanged(old, next api.HistoryItem) bool {
	return old.Kind != next.Kind || old.TotalBytes != next.TotalBytes || !slices.Equal(old.Parts, next.Parts)
}

func (l *liveState) startBusy(notice string) {
	l.busy = true
	l.busySince = time.Now()
	l.busyNotice = notice
}

func (l *liveState) stopBusy() {
	l.busy = false
	l.busySince = time.Time{}
	l.busyNotice = ""
}

func (l *liveState) showBusy() bool {
	if !l.busy {
		return false
	}
	if l.busySince.IsZero() {
		return true
	}
	return time.Since(l.busySince) >= foregroundLoadingIndicatorDelay
}

func (d *viewState) actionBarStatus() string {
	if d.live != nil && d.live.showBusy() && d.live.busyNotice != "" {
		return d.tr(d.live.busyNotice)
	}
	if d.notice != "" {
		return d.tr(d.notice)
	}
	return ""
}

// mutate queues one user intent. Writes are never automatically replayed.
func (d *viewState) mutate(work func(context.Context) error, success string) {
	d.mutateWithin(work, success, 15*time.Second)
}
func (d *viewState) mutateWithin(work func(context.Context) error, success string, budget time.Duration) {
	l := d.live
	if l.busy {
		return
	}
	selected := d.selected
	l.invalidateSearch()
	l.startBusy("正在处理…")
	l.message = ""
	if !l.enqueueWithin(func(ctx context.Context) func() {
		err := work(ctx)
		return func() {
			l.stopBusy()
			l.nextRefresh = time.Time{}
			if err != nil {
				l.message = liveErrorMessage(err)
				d.notice = "操作失败"
				d.toastError(d.tr(l.message), desktopkit.ToastOptions{ID: toastIDLiveMessage})
			} else {
				l.message = ""
				d.notice = success
			}
			if d.selected != selected {
				d.notice = ""
			}
		}
	}, budget) {
		l.stopBusy()
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
	d.mutate(d.live.copyHistory(id, request), "已复制")
}
func (l *liveState) copyHistory(id, request string) func(context.Context) error {
	if l.clipboardUnavailable != nil && l.clipboardUnavailable() {
		return func(ctx context.Context) error { return l.copyFallback(ctx, id) }
	}
	return func(ctx context.Context) error {
		_, err := l.client.RestoreHistory(ctx, id, request)
		return err
	}
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
	tags = withFavoriteTag(tags, slices.Contains(it.Tags, plugin.TagFavorite))
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
			d.toastWarning(d.tr(d.live.message), desktopkit.ToastOptions{ID: toastIDLiveMessage})
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
	d.saveLiveConfigThen(cfg, nil)
}
func (d *viewState) saveLiveConfigThen(cfg config.Config, after func()) {
	d.saveLiveConfigTOMLThen(cfg, false, after)
}

func (d *viewState) clearLiveRelayAccessKey(cfg config.Config, after func()) {
	d.saveLiveConfigTOMLThen(cfg, true, after)
}

func (d *viewState) saveLiveConfigTOMLThen(cfg config.Config, clearRelayAccessKey bool, after func()) {
	revision := d.live.config.Revision
	client := d.live.client
	d.mutate(func(ctx context.Context) error {
		var raw []byte
		if clearRelayAccessKey {
			cfg.Sync.Relay.AccessKey = ""
			var err error
			raw, err = config.Set(cfg, "sync.relay.access_key", `""`)
			if err != nil {
				return err
			}
		} else {
			var buf bytes.Buffer
			if err := toml.NewEncoder(&buf).Encode(cfg); err != nil {
				return err
			}
			raw = buf.Bytes()
		}
		state, err := client.SaveConfig(ctx, revision, raw)
		if err == nil {
			// Publish the saved snapshot before mutate enables another write.
			// Waiting for a later history refresh can expose the old switch
			// value and revision to a second user intent.
			select {
			case d.live.results <- func() {
				d.live.config = state
				d.projectLiveConfig()
				if after != nil && state.ApplyError == "" {
					after()
				}
			}:
				d.live.wake()
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		if err == nil && state.ApplyError != "" {
			return &api.Error{Message: "saved but not applied", Reason: "config_apply_failed"}
		}
		return err
	}, "设置已保存")
}
func (d *viewState) projectLiveConfig() {
	c := d.live.config.Config
	d.theme = c.Desktop.ColorMode
	d.options["startup"] = c.Desktop.LaunchAtLogin
	d.options["sync"] = c.Sync.Enabled
	d.options["autoSync"] = c.Sync.Automatic
	d.options["restore"] = c.Sync.OnRestore
	d.options["telemetry"] = c.Telemetry.Enabled
	if c.Sync.Relay.URL != d.relay.urlSynced {
		d.relay.urlDraft = c.Sync.Relay.URL
		d.relay.urlSynced = c.Sync.Relay.URL
		d.relay.urlErr = ""
	}
}
func (d *viewState) newLiveCode() {
	if d.live.busy || d.live.pairingBackend == "" {
		return
	}
	d.live.codeAttempted = true
	client := d.live.client
	backend := d.live.pairingBackend
	d.mutate(func(ctx context.Context) error {
		code, err := client.NewCodeVia(ctx, backend)
		if err == nil {
			select {
			case d.live.results <- func() { d.live.code = code; d.inviteCode = code.Code }:
				d.live.wake()
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return err
	}, "配对码已生成")
}

func (d *viewState) newLiveTicket() {
	if d.live.busy || d.live.pairingBackend == "" {
		return
	}
	d.live.ticketAttempted = true
	client := d.live.client
	backend := d.live.pairingBackend
	d.mutate(func(ctx context.Context) error {
		ticket, err := client.ExportTicketVia(ctx, backend)
		if err == nil {
			select {
			case d.live.results <- func() { d.live.ticket = ticket }:
				d.live.wake()
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

func loadSearchEntry(ctx context.Context, client *api.Client, it api.HistoryItem, edge int) entry {
	id := it.ID
	v := projectHistory(it)
	fileCount := 0
	if it.Kind == "files" {
		for _, p := range it.Parts {
			if p.Kind == "file" {
				fileCount++
			}
		}
	}
	// Single-file MIME/name may only become known after the response opens.
	// Admit those possible image previews conservatively, without gating text.
	if it.Kind == "image" || (it.Kind == "files" && fileCount == 1) {
		if err := imageRequests.acquire(ctx, edge == 0); err != nil {
			v.contentError = "读取内容失败。"
			return v
		}
		defer imageRequests.release()
		if ctx.Err() != nil {
			v.contentError = "读取内容失败。"
			return v
		}
	}
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
			imageFile := strings.HasPrefix(r.MIME, "image/") || isImageFilename(r.Name)
			if fileCount == 1 && imageFile && p.Size <= 32<<20 && r.Size <= 32<<20 {
				v.picture, _ = readPreviewImage(ctx, r, edge)
				if v.picture != nil {
					v.image = v.picture.bitmap
				}
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
			v.picture, v.contentError = readPreviewImage(ctx, r, edge)
			if v.picture != nil {
				v.image = v.picture.bitmap
			}
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
		} else if slices.Contains(it.Tags, plugin.TagLink) {
			v.linkPreview = loadLinkPreview(ctx, client, id, edge)
		}
		v.title = shortText(strings.TrimSpace(v.text), 90)
		v.generatedTitle = false
	}
	return v
}

func (d *viewState) findLiveAction(key string) (api.Action, bool) {
	if d.live == nil {
		return api.Action{}, false
	}
	for _, a := range d.live.actions {
		if a.Key == key {
			return a, true
		}
	}
	return api.Action{}, false
}

func (d *viewState) runHistoryAction(id, key string) {
	l := d.live
	if l == nil || l.busy {
		return
	}
	act, ok := d.findLiveAction(key)
	if !ok {
		act = api.Action{Key: key, Title: key, PostActions: []string{plugin.ActionPostCopy, plugin.ActionPostNotify}}
	}
	wantCopy := slices.Contains(act.PostActions, plugin.ActionPostCopy)
	wantNotify := slices.Contains(act.PostActions, plugin.ActionPostNotify)
	wantPresent := slices.Contains(act.PostActions, plugin.ActionPostPresent)
	wantSelect := slices.Contains(act.PostActions, plugin.ActionPostSelect)

	title := d.tr(act.Title)
	pluginLabel := d.actionPluginLabel(act.PluginID)
	client := l.client
	selected := d.selected
	request := api.ActionInput{
		RequestID:       "desktop-" + newRequestID(),
		Apply:           wantCopy,
		CancelOnContext: true,
	}

	l.invalidateSearch()
	l.startBusy("正在处理…")
	l.message = ""
	d.toasts.Info(fmt.Sprintf(d.tr("正在执行「%s」…"), title), desktopkit.ToastOptions{
		ID:             toastIDActionRun,
		Loading:        true,
		Duration:       desktopkit.ToastPersistent,
		NonDismissible: true,
		Actions: []desktopkit.ToastAction{{
			Label:    d.tr("取消"),
			Variant:  "secondary",
			KeepOpen: true,
			OnClick:  d.cancelRunningAction,
		}},
	})
	actionCtx, actionCancel := context.WithCancel(l.ctx)
	l.actionCancel = actionCancel

	const presentMaxBytes = 64 << 10
	queued := l.enqueueWithin(func(ctx context.Context) func() {
		callCtx, stopCancel := context.WithCancel(ctx)
		stopLink := context.AfterFunc(actionCtx, stopCancel)
		defer func() {
			stopLink()
			stopCancel()
		}()

		result, err := client.ExecuteAction(callCtx, id, key, request)
		var resultText string
		var resultURL string
		var qrImg *image.NRGBA
		var truncated bool
		if err == nil && wantPresent && result.Item.ID != "" {
			if rc, cerr := client.HistoryContent(callCtx, result.Item.ID, 0); cerr == nil {
				raw, rerr := io.ReadAll(io.LimitReader(rc, presentMaxBytes+1))
				_ = rc.Close()
				if rerr == nil {
					if len(raw) > presentMaxBytes {
						raw = raw[:presentMaxBytes]
						truncated = true
					}
					resultText = strings.ToValidUTF8(string(raw), "")
					resultURL = validHTTPURL(resultText)
					qrImg = encodeActionQRImage(resultURL)
				}
			}
		}

		return func() {
			cancelledByUser := actionCtx.Err() != nil && l.ctx.Err() == nil
			actionCancel()
			l.actionCancel = nil
			l.stopBusy()
			l.nextRefresh = time.Time{}

			if err != nil {
				if cancelledByUser {
					l.message = ""
					d.notice = ""
					if wantNotify {
						d.toasts.Update(toastIDActionRun, fmt.Sprintf(d.tr("已取消「%s」"), title), desktopkit.ToastOptions{
							Status:   desktopkit.ToastWarning,
							Duration: 3 * time.Second,
						})
					} else {
						d.toastDismiss(toastIDActionRun)
					}
					return
				}
				l.message = liveErrorMessage(err)
				d.notice = "操作失败"
				d.toasts.Update(toastIDActionRun, fmt.Sprintf(d.tr("「%s」执行失败"), title), desktopkit.ToastOptions{
					Description: d.tr(l.message),
					Status:      desktopkit.ToastError,
					Duration:    4 * time.Second,
				})
				return
			}

			applyFailed := wantCopy && !result.Applied
			if wantSelect && result.Item.ID != "" {
				d.focusHistoryItem(result.Item.ID)
			}

			if wantPresent {
				if applyFailed {
					l.message = ""
					d.notice = ""
					d.toasts.Update(toastIDActionRun, d.tr("已保存到历史，但未能写入剪贴板。"), desktopkit.ToastOptions{
						Status:   desktopkit.ToastWarning,
						Duration: 4 * time.Second,
					})
				} else {
					d.toastDismiss(toastIDActionRun)
					if wantCopy {
						l.message = ""
						d.notice = "已复制"
					} else {
						l.message = ""
						d.notice = ""
					}
				}
				d.actionResult = actionResultState{
					open:        true,
					title:       title,
					plugin:      pluginLabel,
					historyID:   result.Item.ID,
					text:        resultText,
					url:         resultURL,
					qr:          newActionResultQR(qrImg),
					copied:      wantCopy && result.Applied,
					applyFailed: applyFailed,
					truncated:   truncated,
				}
				if d.selected != selected && !wantSelect {
					d.notice = ""
				}
				return
			}

			if applyFailed {
				l.message = liveErrorMessage(&api.Error{Reason: "action_apply_failed", Message: "action saved to history"})
				d.notice = "操作失败"
				d.toasts.Update(toastIDActionRun, d.tr("已保存到历史，但未能写入剪贴板。"), desktopkit.ToastOptions{
					Status:   desktopkit.ToastWarning,
					Duration: 4 * time.Second,
				})
				return
			}

			l.message = ""
			if wantCopy {
				d.notice = "已复制"
			} else {
				d.notice = ""
			}
			if d.selected != selected && !wantSelect {
				d.notice = ""
			}
			if wantNotify {
				msg := fmt.Sprintf(d.tr("已完成「%s」"), title)
				if wantCopy {
					msg = fmt.Sprintf(d.tr("已完成「%s」· 已复制"), title)
				}
				d.toasts.Update(toastIDActionRun, msg, desktopkit.ToastOptions{
					Status:   desktopkit.ToastSuccess,
					Duration: 4 * time.Second,
				})
			} else {
				d.toastDismiss(toastIDActionRun)
			}
		}
	}, 5*time.Minute+30*time.Second)
	if !queued {
		actionCancel()
		l.actionCancel = nil
		l.stopBusy()
		d.toastDismiss(toastIDActionRun)
	}
}
