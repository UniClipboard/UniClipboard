package desktopui

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
	"github.com/uniclipboard/unic/app/desktop/internal/announcements"
)

func TestSidebarFlareReleaseAnnouncements(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}

	t.Run("announcement with actionUrl aligns to sidebar grid, opens URL and persists ID", func(t *testing.T) {
		for _, theme := range []string{"light", "dark"} {
			t.Run(theme, func(t *testing.T) {
				d := newDemo("history", theme)
				desktop := &Desktop{d: d}
				var openedURL string
				var dismissedIDs []string
				desktop.SetOpenExternal(func(u string) error {
					openedURL = u
					return nil
				})
				desktop.SetDismissedAnnouncements(nil, func(id string) {
					dismissedIDs = append(dismissedIDs, id)
				})
				desktop.SetRemoteAnnouncements([]announcements.Announcement{
					{
						ID:          "v2-release",
						Severity:    "info",
						Title:       "UniClipboard 2.0 全新发布",
						Content:     "探索 2.0 全新功能与体验升级。",
						ActionURL:   "https://www.uniclipboard.app/blog/v2",
						PublishedAt: "2026-11-25T00:00:00Z",
					},
				})

				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
					Dark:        theme == "dark",
					Preferences: ui.Preferences{ReduceMotion: true},
				})
				tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})

				card, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.0 全新发布")
				if !ok {
					t.Fatal("announcement card missing from sidebar")
				}
				allRow, ok := tt.AccessByRole(ui.RoleListItem, "全部项目")
				if !ok {
					t.Fatal("all items row missing")
				}
				footerRow, ok := tt.AccessByRole(ui.RoleListItem, "管理设备…")
				if !ok {
					t.Fatal("footer row missing")
				}
				if card.Bounds.X != allRow.Bounds.X || card.Bounds.W != allRow.Bounds.W {
					t.Fatalf("announcement alignment mismatch: card=%+v row=%+v", card.Bounds, allRow.Bounds)
				}
				if card.Bounds.Y+card.Bounds.H > footerRow.Bounds.Y {
					t.Fatalf("announcement overlaps footer: card=%+v footer=%+v", card.Bounds, footerRow.Bounds)
				}

				if err := tt.Click("查看详情"); err != nil {
					t.Fatalf("click remote CTA: %v", err)
				}
				tt.Frame()
				if openedURL != "https://www.uniclipboard.app/blog/v2" {
					t.Fatalf("openedURL=%q want v2 blog URL", openedURL)
				}
				if len(dismissedIDs) != 1 || dismissedIDs[0] != "v2-release" {
					t.Fatalf("dismissedIDs=%v want [v2-release]", dismissedIDs)
				}
				if _, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.0 全新发布"); ok {
					t.Fatal("remote announcement remained visible after CTA click")
				}
			})
		}
	})

	t.Run("close button dismisses without opening URL and open error keeps card visible", func(t *testing.T) {
		d := newDemo("history", "light")
		desktop := &Desktop{d: d}
		opened := false
		var dismissedIDs []string
		desktop.SetOpenExternal(func(string) error {
			opened = true
			return errors.New("browser unavailable")
		})
		desktop.SetDismissedAnnouncements(nil, func(id string) {
			dismissedIDs = append(dismissedIDs, id)
		})
		desktop.SetRemoteAnnouncements([]announcements.Announcement{
			{
				ID:          "v2-blog",
				Severity:    "info",
				Title:       "What's new in 2.0",
				Content:     "Explore 2.0 features.",
				ActionURL:   "https://www.uniclipboard.app/blog/v2",
				PublishedAt: "2026-11-25T00:00:00Z",
			},
		})
		d.language = "en"

		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
			Preferences: ui.Preferences{ReduceMotion: true},
		})
		if err := tt.Click("View details"); err != nil {
			t.Fatalf("click english CTA: %v", err)
		}
		tt.Frame()
		if !opened || len(dismissedIDs) != 0 || d.notice != "Could not open the link." {
			t.Fatalf("open error state mismatch: opened=%v dismissed=%v notice=%q", opened, dismissedIDs, d.notice)
		}
		if _, ok := tt.AccessByRole(ui.RoleRegion, "What's new in 2.0"); !ok {
			t.Fatal("card should remain visible after URL open failure")
		}

		opened = false
		if err := tt.Click("Close"); err != nil {
			t.Fatalf("click close button: %v", err)
		}
		tt.Frame()
		if opened {
			t.Fatal("close button should not open URL")
		}
		if len(dismissedIDs) != 1 || dismissedIDs[0] != "v2-blog" {
			t.Fatalf("dismissedIDs=%v want [v2-blog]", dismissedIDs)
		}
		if _, ok := tt.AccessByRole(ui.RoleRegion, "What's new in 2.0"); ok {
			t.Fatal("card remained visible after close button")
		}
	})

	t.Run("remote announcement without actionUrl opens detail sheet and dismisses from footer", func(t *testing.T) {
		d := newDemo("history", "dark")
		desktop := &Desktop{d: d}
		var dismissedIDs []string
		desktop.SetDismissedAnnouncements(nil, func(id string) {
			dismissedIDs = append(dismissedIDs, id)
		})
		desktop.SetRemoteAnnouncements([]announcements.Announcement{
			{
				ID:          "maint-1",
				Severity:    "warning",
				Title:       "中继节点维护通知",
				Content:     "## 维护窗口\n**今晚 23:00** 将进行短时升级。",
				PublishedAt: "2026-11-25T00:00:00Z",
			},
		})

		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
			Dark:        true,
			Preferences: ui.Preferences{ReduceMotion: true},
		})
		if err := tt.Click("查看详情"); err != nil {
			t.Fatalf("click detail CTA: %v", err)
		}
		tt.Frame()
		if !d.announcement.detailOpen {
			t.Fatal("announcement detail sheet did not open")
		}
		if !tt.HasText("注意事项") || !tt.HasText("维护窗口") {
			t.Fatalf("detail sheet missing severity or stripped content: %v", tt.Texts())
		}
		if err := tt.Click("不再显示"); err != nil {
			t.Fatalf("click dismiss in sheet: %v", err)
		}
		tt.Advance(350 * time.Millisecond)
		tt.Frame()
		if d.announcement.detailOpen {
			t.Fatal("detail sheet did not close")
		}
		if len(dismissedIDs) != 1 || dismissedIDs[0] != "maint-1" {
			t.Fatalf("dismissedIDs=%v want [maint-1]", dismissedIDs)
		}
		if _, ok := tt.AccessByRole(ui.RoleRegion, "中继节点维护通知"); ok {
			t.Fatal("remote announcement still visible after sheet dismissal")
		}
	})

	t.Run("priority: urgent beats info, dismissed IDs skipped, next queued announcement appears on dismiss", func(t *testing.T) {
		d := newDemo("history", "light")
		desktop := &Desktop{d: d}
		desktop.SetDismissedAnnouncements([]string{"already-dismissed"}, nil)
		desktop.SetRemoteAnnouncements([]announcements.Announcement{
			{ID: "already-dismissed", Severity: "critical", Title: "Ignored", Content: "x", PublishedAt: "2026-11-25T00:00:00Z"},
			{ID: "info-1", Severity: "info", Title: "Routine update", Content: "Details", ActionURL: "https://www.uniclipboard.app/blog/info", PublishedAt: "2026-11-25T00:00:00Z"},
			{ID: "crit-1", Severity: "critical", Title: "Urgent security fix", Content: "Upgrade now", PublishedAt: "2026-11-25T00:00:00Z"},
		})

		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
			Preferences: ui.Preferences{ReduceMotion: true},
		})
		if _, ok := tt.AccessByRole(ui.RoleRegion, "Urgent security fix"); !ok {
			t.Fatal("critical announcement should appear ahead of routine info")
		}
		if err := tt.Click("关闭"); err != nil {
			t.Fatalf("close critical announcement: %v", err)
		}
		tt.Frame()
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleRegion, "Routine update"); !ok {
			t.Fatal("queued info announcement did not appear after critical dismissed")
		}
	})

	t.Run("no card rendered when FlareRelease returns empty list", func(t *testing.T) {
		d := newDemo("history", "light")
		desktop := &Desktop{d: d}
		desktop.SetRemoteAnnouncements(nil)
		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{})
		if _, ok := tt.AccessByRole(ui.RoleButton, "查看详情"); ok {
			t.Fatal("unexpected announcement button when feed is empty")
		}
	})

	t.Run("live session fetches FlareRelease announcements asynchronously and binds query params", func(t *testing.T) {
		desktop, d := newTestAboutLive(t)
		d.page = "history"
		var mu sync.Mutex
		var calls []string
		desktop.SetAnnouncementFetcher(func(_ context.Context, version, channel, locale string) ([]announcements.Announcement, error) {
			mu.Lock()
			calls = append(calls, version+"|"+channel+"|"+locale)
			mu.Unlock()
			return []announcements.Announcement{
				{
					ID:          "live-1",
					Severity:    "info",
					Title:       "Live FlareRelease Notice",
					Content:     "Synced from server",
					ActionURL:   "https://www.uniclipboard.app/blog/v2",
					PublishedAt: "2026-11-25T00:00:00Z",
				},
			}, nil
		})
		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
			Preferences: ui.Preferences{ReduceMotion: true},
		})
		deadline := time.Now().Add(2 * time.Second)
		for time.Now().Before(deadline) {
			tt.Frame()
			if _, ok := tt.AccessByRole(ui.RoleRegion, "Live FlareRelease Notice"); ok {
				break
			}
			time.Sleep(10 * time.Millisecond)
		}
		if _, ok := tt.AccessByRole(ui.RoleRegion, "Live FlareRelease Notice"); !ok {
			t.Fatal("live fetched announcement did not render in sidebar")
		}
		mu.Lock()
		defer mu.Unlock()
		if len(calls) != 1 || calls[0] != "2.0.0|stable|zh-CN" {
			t.Fatalf("unexpected fetch calls: %v", calls)
		}
	})
}
