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
	announcementRefreshPeriod  = 5 * time.Minute
	announcementSummaryMaxRune = 84
)

type announcementFetcher func(ctx context.Context, version, channel, locale string) ([]announcements.Announcement, error)

type announcementState struct {
	remote          []announcements.Announcement
	remoteLoaded    bool
	dismissedIDs    map[string]bool
	onDismissRemote func(string)
	fetchRemote     announcementFetcher
	fetchInFlight   bool
	fetchLocale     string
	fetchChannel    string
	lastFetch       time.Time

	// displayedItem pins the FlareRelease card currently mounted in the sidebar
	// so its exit animation finishes cleanly before any next announcement mounts.
	displayed     bool
	displayedItem announcements.Announcement
	cardOpen      bool

	detailOpen bool
	detailItem announcements.Announcement
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

// nextSidebarAnnouncement picks the highest-priority undismissed FlareRelease
// announcement (critical/warning ahead of info, preserving server order within
// each tier).
func (d *viewState) nextSidebarAnnouncement() (announcements.Announcement, bool) {
	var first *announcements.Announcement
	for i := range d.announcement.remote {
		item := &d.announcement.remote[i]
		if d.isRemoteDismissed(item.ID) {
			continue
		}
		if item.Severity == "critical" || item.Severity == "warning" {
			return *item, true
		}
		if first == nil {
			first = item
		}
	}
	if first != nil {
		return *first, true
	}
	return announcements.Announcement{}, false
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
	if !a.displayed {
		if item, ok := d.nextSidebarAnnouncement(); ok {
			a.displayed = true
			a.displayedItem = item
			a.cardOpen = true
		}
	} else if d.isRemoteDismissed(a.displayedItem.ID) {
		a.cardOpen = false
	} else if updated, ok := d.findRemoteAnnouncement(a.displayedItem.ID); ok {
		a.displayedItem = updated
	} else {
		a.cardOpen = false
	}

	var props desktopkit.AnnouncementProps
	if a.displayed {
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
	} else {
		closed := false
		props = desktopkit.AnnouncementProps{Open: &closed}
	}

	res := k.Announcement(c, props)
	if res.ActionClicked && a.displayed {
		item := a.displayedItem
		if item.ActionURL != "" {
			if err := d.openExternalURL(item.ActionURL); err != nil {
				msg := d.tr("无法打开链接。")
				d.notice = msg
				d.toastError(msg)
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
	if res.Dismissed && a.displayed {
		d.persistRemoteAnnouncementDismissed(a.displayedItem.ID)
	}
	if res.Closed {
		a.displayed = false
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
				if a.displayed && a.displayedItem.ID == item.ID {
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
				label, color := d.tr("发布公告"), desktopkit.TagColor("blue")
				switch item.Severity {
				case "critical":
					label, color = d.tr("重要公告"), desktopkit.TagColor("red")
				case "warning":
					label, color = d.tr("注意事项"), desktopkit.TagColor("orange")
				}
				k.Tag(c.Key("severity"), label, desktopkit.TagProps{Color: color})
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
