package desktopui

import (
	"context"
	"regexp"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/announcements"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

const (
	v2AnnouncementURL          = "https://www.uniclipboard.app/blog/v2"
	announcementRefreshPeriod  = 5 * time.Minute
	announcementSummaryMaxRune = 84
)

type announcementFetcher func(ctx context.Context, version, channel, locale string) ([]announcements.Announcement, error)

type announcementState struct {
	v2Open      bool
	v2Dismissed bool
	onDismissV2 func()

	remote          []announcements.Announcement
	remoteLoaded    bool
	dismissedIDs    map[string]bool
	onDismissRemote func(string)
	fetchRemote     announcementFetcher
	fetchInFlight   bool
	fetchLocale     string
	fetchChannel    string
	lastFetch       time.Time

	displayedKind string // "", "v2", "remote"
	displayedItem announcements.Announcement
	cardOpen      bool

	detailOpen bool
	detailItem announcements.Announcement
}

// SetV2Announcement configures the first-install v2 feature announcement card
// displayed on the history sidebar. Call on the view thread before rendering.
func (s *Desktop) SetV2Announcement(visible bool, onDismiss func()) {
	s.d.announcement.v2Open = visible
	s.d.announcement.v2Dismissed = !visible
	s.d.announcement.onDismissV2 = onDismiss
}

// SetDismissedAnnouncements restores previously dismissed FlareRelease
// announcement IDs and binds the persistence callback for future dismissals.
func (s *Desktop) SetDismissedAnnouncements(ids []string, onDismiss func(id string)) {
	m := make(map[string]bool, len(ids))
	for _, id := range ids {
		if id = strings.TrimSpace(id); id != "" {
			m[id] = true
		}
	}
	s.d.announcement.dismissedIDs = m
	s.d.announcement.onDismissRemote = onDismiss
}

// SetRemoteAnnouncements supplies FlareRelease announcements directly (used by
// previews and deterministic tests). Live sessions fetch via FlareRelease HTTP.
func (s *Desktop) SetRemoteAnnouncements(items []announcements.Announcement) {
	s.d.announcement.remote = slices.Clone(items)
	s.d.announcement.remoteLoaded = true
}

// SetAnnouncementFetcher overrides how live desktop sessions fetch FlareRelease
// announcements. Nil uses the production HTTPS FlareRelease endpoint.
func (s *Desktop) SetAnnouncementFetcher(fetch func(ctx context.Context, version, channel, locale string) ([]announcements.Announcement, error)) {
	s.d.announcement.fetchRemote = fetch
}

func (d *viewState) persistV2AnnouncementDismissed() {
	d.announcement.v2Open = false
	if d.announcement.v2Dismissed {
		return
	}
	d.announcement.v2Dismissed = true
	if d.announcement.onDismissV2 != nil {
		d.announcement.onDismissV2()
	}
}

func (d *viewState) persistRemoteAnnouncementDismissed(id string) {
	id = strings.TrimSpace(id)
	if id == "" {
		return
	}
	if d.announcement.dismissedIDs == nil {
		d.announcement.dismissedIDs = map[string]bool{}
	}
	if d.announcement.dismissedIDs[id] {
		return
	}
	d.announcement.dismissedIDs[id] = true
	if d.announcement.onDismissRemote != nil {
		d.announcement.onDismissRemote(id)
	}
}

func (d *viewState) isRemoteDismissed(id string) bool {
	return d.announcement.dismissedIDs != nil && d.announcement.dismissedIDs[id]
}

func (d *viewState) findRemoteAnnouncement(id string) (announcements.Announcement, bool) {
	for _, item := range d.announcement.remote {
		if item.ID == id {
			return item, true
		}
	}
	return announcements.Announcement{}, false
}

// nextSidebarAnnouncement resolves which announcement should occupy the single
// sidebar slot: urgent remote (critical/warning) > local first-install v2 >
// routine remote (info).
func (d *viewState) nextSidebarAnnouncement() (string, announcements.Announcement) {
	var firstRemote *announcements.Announcement
	for i := range d.announcement.remote {
		item := &d.announcement.remote[i]
		if d.isRemoteDismissed(item.ID) {
			continue
		}
		if item.Severity == "critical" || item.Severity == "warning" {
			return "remote", *item
		}
		if firstRemote == nil {
			firstRemote = item
		}
	}
	if d.announcement.v2Open {
		return "v2", announcements.Announcement{}
	}
	if firstRemote != nil {
		return "remote", *firstRemote
	}
	return "", announcements.Announcement{}
}

func (d *viewState) ensureRemoteAnnouncementsLoaded() {
	if d.live == nil || !d.live.loaded || d.announcement.fetchInFlight {
		return
	}
	locale := d.currentLanguage()
	ver, _ := d.aboutVersionParts()
	channel := parseAboutChannel(ver, d.currentUpdateChannel())
	needsFetch := !d.announcement.remoteLoaded ||
		d.announcement.fetchLocale != locale ||
		d.announcement.fetchChannel != channel ||
		time.Since(d.announcement.lastFetch) >= announcementRefreshPeriod
	if !needsFetch {
		return
	}
	d.announcement.fetchInFlight = true
	d.announcement.fetchLocale = locale
	d.announcement.fetchChannel = channel
	d.announcement.lastFetch = time.Now()

	fetcher := d.announcement.fetchRemote
	if fetcher == nil {
		fetcher = func(ctx context.Context, version, ch, loc string) ([]announcements.Announcement, error) {
			return announcements.Fetch(ctx, nil, "", version, ch, loc)
		}
	}
	l := d.live
	go func() {
		items, err := fetcher(l.ctx, ver, channel, locale)
		apply := func() {
			d.announcement.fetchInFlight = false
			if d.announcement.fetchLocale != locale || d.announcement.fetchChannel != channel {
				return
			}
			d.announcement.remoteLoaded = true
			if err == nil {
				d.announcement.remote = items
			}
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

func (d *viewState) sidebarAnnouncement(c *ui.Context, k desktopkit.Kit) desktopkit.AnnouncementResult {
	d.ensureRemoteAnnouncementsLoaded()

	a := &d.announcement
	switch a.displayedKind {
	case "":
		if kind, item := d.nextSidebarAnnouncement(); kind != "" {
			a.displayedKind = kind
			a.displayedItem = item
			a.cardOpen = true
		}
	case "v2":
		if !a.v2Open {
			a.cardOpen = false
		}
	case "remote":
		if d.isRemoteDismissed(a.displayedItem.ID) {
			a.cardOpen = false
		} else if updated, ok := d.findRemoteAnnouncement(a.displayedItem.ID); ok {
			a.displayedItem = updated
		} else {
			a.cardOpen = false
		}
	}

	var props desktopkit.AnnouncementProps
	switch a.displayedKind {
	case "v2":
		props = desktopkit.AnnouncementProps{
			Open:        &a.cardOpen,
			Title:       d.tr("UniClipboard 2.0 全新发布"),
			Description: d.tr("探索 2.0 全新功能与体验升级。"),
			ActionLabel: d.tr("查看功能介绍"),
			Dismissible: true,
			CloseLabel:  d.tr("关闭"),
			IntroDelay:  300 * time.Millisecond,
		}
	case "remote":
		item := a.displayedItem
		var icon desktopkit.IconName
		if item.Severity == "critical" || item.Severity == "warning" {
			icon = desktopkit.IconWarning
		}
		actionLabel := ""
		if item.ActionURL != "" || strings.TrimSpace(item.Content) != "" {
			actionLabel = d.tr("查看详情")
		}
		props = desktopkit.AnnouncementProps{
			Open:        &a.cardOpen,
			Title:       item.Title,
			Description: announcementSummary(item.Content),
			Icon:        icon,
			ActionLabel: actionLabel,
			Dismissible: true,
			CloseLabel:  d.tr("关闭"),
			IntroDelay:  300 * time.Millisecond,
		}
	default:
		closed := false
		props = desktopkit.AnnouncementProps{Open: &closed}
	}

	res := k.Announcement(c, props)
	if res.ActionClicked {
		switch a.displayedKind {
		case "v2":
			if err := d.openExternalURL(v2AnnouncementURL); err != nil {
				d.notice = d.tr("无法打开链接。")
			} else {
				d.persistV2AnnouncementDismissed()
				a.cardOpen = false
				c.Invalidate()
			}
		case "remote":
			item := a.displayedItem
			if item.ActionURL != "" {
				if err := d.openExternalURL(item.ActionURL); err != nil {
					d.notice = d.tr("无法打开链接。")
				} else {
					d.persistRemoteAnnouncementDismissed(item.ID)
					a.cardOpen = false
					c.Invalidate()
				}
			} else if strings.TrimSpace(item.Content) != "" {
				a.detailItem = item
				a.detailOpen = true
				c.Invalidate()
			}
		}
	}
	if res.Dismissed {
		switch a.displayedKind {
		case "v2":
			d.persistV2AnnouncementDismissed()
		case "remote":
			d.persistRemoteAnnouncementDismissed(a.displayedItem.ID)
		}
	}
	if res.Closed {
		a.displayedKind = ""
		a.displayedItem = announcements.Announcement{}
		c.Invalidate()
	}
	return res
}

func (d *viewState) announcementDetailSheet(c *ui.Context, k desktopkit.Kit) {
	a := &d.announcement
	item := a.detailItem
	done := k.Sheet(c, desktopkit.SheetProps{
		Variant: desktopkit.SheetCompact,
		Open:    &a.detailOpen,
		Title:   item.Title,
		Done:    d.tr("完成"),
		Footer: func() {
			if item.ID != "" && k.Button(c.Key("dismiss"), d.tr("不再显示"), desktopkit.ButtonProps{Variant: desktopkit.Subtle}).Clicked() {
				d.persistRemoteAnnouncementDismissed(item.ID)
				if a.displayedKind == "remote" && a.displayedItem.ID == item.ID {
					a.cardOpen = false
				}
				a.detailOpen = false
				c.Invalidate()
			}
		},
		OnClosed: func() {
			a.detailItem = announcements.Announcement{}
		},
	}, func() {
		ui.Column(c.Key("body")).FillWidth().Gap(12).Children(func() {
			ui.Row(c.Key("meta")).Gap(8).AlignItems(ui.Center).Children(func() {
				label, tone := d.tr("发布公告"), desktopkit.ToneInfo
				switch item.Severity {
				case "critical":
					label, tone = d.tr("重要公告"), desktopkit.ToneError
				case "warning":
					label, tone = d.tr("注意事项"), desktopkit.ToneWarning
				}
				k.Tag(c.Key("severity"), label, desktopkit.TagProps{Color: noticeToneColor(tone)})
				if pub := formatAnnouncementDate(item.PublishedAt); pub != "" {
					k.Text(c.Key("date"), pub, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
				}
			})
			if content := strings.TrimSpace(item.Content); content != "" {
				k.Text(c.Key("content"), stripAnnouncementMarkdown(content), desktopkit.TextProps{Role: desktopkit.RoleBody})
			}
		})
	})
	if done {
		a.detailOpen = false
	}
}

func noticeToneColor(tone desktopkit.NoticeTone) desktopkit.TagColor {
	switch tone {
	case desktopkit.ToneError:
		return "red"
	case desktopkit.ToneWarning:
		return "orange"
	default:
		return "blue"
	}
}

func formatAnnouncementDate(raw string) string {
	t, err := time.Parse(time.RFC3339, strings.TrimSpace(raw))
	if err != nil {
		return ""
	}
	return t.Local().Format("2006-01-02")
}

var (
	reMDLink   = regexp.MustCompile(`\[([^\]]+)\]\([^)]+\)`)
	reMDPrefix = regexp.MustCompile(`(?m)^\s{0,3}(?:#{1,6}\s+|[-*+]\s+|\d+\.\s+|>\s+)`)
)

func stripAnnouncementMarkdown(raw string) string {
	s := strings.ReplaceAll(raw, "\r\n", "\n")
	s = reMDLink.ReplaceAllString(s, "$1")
	s = reMDPrefix.ReplaceAllString(s, "")
	s = strings.NewReplacer("**", "", "__", "", "`", "", "*", "").Replace(s)
	return strings.TrimSpace(s)
}

func announcementSummary(content string) string {
	s := stripAnnouncementMarkdown(content)
	if s == "" {
		return ""
	}
	s = strings.Join(strings.Fields(s), " ")
	runes := []rune(s)
	if len(runes) <= announcementSummaryMaxRune {
		return s
	}
	return strings.TrimSpace(string(runes[:announcementSummaryMaxRune-1])) + "…"
}
