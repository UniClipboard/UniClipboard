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

func TestSidebarV2AnnouncementOpensFixedURLAndDismisses(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	for _, theme := range []string{"light", "dark"} {
		t.Run(theme, func(t *testing.T) {
			d := newDemo("history", theme)
			desktop := &Desktop{d: d}
			var openedURL string
			dismissed := 0
			desktop.SetOpenExternal(func(u string) error {
				openedURL = u
				return nil
			})
			desktop.SetV2Announcement(true, func() {
				dismissed++
			})

			tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
				Dark:        theme == "dark",
				Preferences: ui.Preferences{ReduceMotion: true},
			})
			tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})

			card, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.0 全新发布")
			if !ok {
				t.Fatal("v2 announcement card missing from sidebar")
			}
			allRow, ok := tt.AccessByRole(ui.RoleListItem, "全部项目")
			if !ok {
				t.Fatal("all items row missing")
			}
			footerRow, ok := tt.AccessByRole(ui.RoleListItem, "管理设备…")
			if !ok {
				t.Fatal("footer row missing")
			}
			// Card must align with the 12-DIP sidebar grid and sit above the fixed footer.
			if card.Bounds.X != allRow.Bounds.X || card.Bounds.W != allRow.Bounds.W {
				t.Fatalf("announcement alignment mismatch: card=%+v row=%+v", card.Bounds, allRow.Bounds)
			}
			if card.Bounds.Y+card.Bounds.H > footerRow.Bounds.Y {
				t.Fatalf("announcement overlaps footer: card=%+v footer=%+v", card.Bounds, footerRow.Bounds)
			}

			if err := tt.Click("查看功能介绍"); err != nil {
				t.Fatalf("click announcement CTA: %v", err)
			}
			tt.Frame()
			if openedURL != "https://www.uniclipboard.app/blog/v2" {
				t.Fatalf("openedURL=%q want https://www.uniclipboard.app/blog/v2", openedURL)
			}
			if dismissed != 1 {
				t.Fatalf("dismissed=%d want 1", dismissed)
			}
			if _, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.0 全新发布"); ok {
				t.Fatal("announcement still visible after CTA click")
			}
		})
	}
}

func TestSidebarV2AnnouncementCloseAndOpenError(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	t.Run("close button dismisses without opening URL", func(t *testing.T) {
		d := newDemo("history", "light")
		desktop := &Desktop{d: d}
		opened := false
		dismissed := 0
		desktop.SetOpenExternal(func(string) error {
			opened = true
			return nil
		})
		desktop.SetV2Announcement(true, func() {
			dismissed++
		})

		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: false})
		tt.Advance(600 * time.Millisecond)

		if err := tt.Click("关闭"); err != nil {
			t.Fatalf("click close button: %v", err)
		}
		if opened {
			t.Fatal("close button should not open URL")
		}
		if dismissed != 1 {
			t.Fatalf("dismissed=%d want 1", dismissed)
		}
		tt.Advance(320 * time.Millisecond)
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.0 全新发布"); ok {
			t.Fatal("announcement remained visible after exit animation")
		}
	})

	t.Run("open URL error keeps announcement visible and sets notice", func(t *testing.T) {
		d := newDemo("history", "light")
		desktop := &Desktop{d: d}
		dismissed := 0
		desktop.SetOpenExternal(func(string) error {
			return errors.New("browser unavailable")
		})
		desktop.SetV2Announcement(true, func() {
			dismissed++
		})

		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
			Dark:        false,
			Preferences: ui.Preferences{ReduceMotion: true},
		})
		if err := tt.Click("查看功能介绍"); err != nil {
			t.Fatalf("click CTA: %v", err)
		}
		tt.Frame()
		if dismissed != 0 {
			t.Fatalf("dismissed=%d want 0 on open error", dismissed)
		}
		if d.notice != "无法打开链接。" {
			t.Fatalf("notice=%q want 无法打开链接。", d.notice)
		}
		if _, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.0 全新发布"); !ok {
			t.Fatal("announcement should remain visible when opening URL fails")
		}
	})

	t.Run("hidden when not first install", func(t *testing.T) {
		d := newDemo("history", "light")
		desktop := &Desktop{d: d}
		desktop.SetV2Announcement(false, nil)
		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: false})
		if _, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.0 全新发布"); ok {
			t.Fatal("announcement should not appear when disabled")
		}
	})

	t.Run("english locale renders translated copy", func(t *testing.T) {
		d := newDemo("history", "light")
		d.language = "en"
		desktop := &Desktop{d: d}
		var openedURL string
		desktop.SetOpenExternal(func(u string) error {
			openedURL = u
			return nil
		})
		desktop.SetV2Announcement(true, nil)
		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
			Dark:        false,
			Preferences: ui.Preferences{ReduceMotion: true},
		})
		if _, ok := tt.AccessByRole(ui.RoleRegion, "What's new in 2.0"); !ok {
			t.Fatal("translated english announcement missing")
		}
		if err := tt.Click("Read what's new"); err != nil {
			t.Fatalf("click english CTA: %v", err)
		}
		if openedURL != "https://www.uniclipboard.app/blog/v2" {
			t.Fatalf("openedURL=%q want v2 blog URL", openedURL)
		}
	})
}

func TestSidebarFlareReleaseAnnouncements(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}

	t.Run("remote announcement with actionUrl opens URL and persists ID", func(t *testing.T) {
		d := newDemo("history", "light")
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
				ID:          "rel-2-1",
				Severity:    "info",
				Title:       "UniClipboard 2.1 发布",
				Content:     "**全新同步引擎**与快捷面板升级。",
				ActionURL:   "https://www.uniclipboard.app/blog/v2-1",
				PublishedAt: "2026-11-25T00:00:00Z",
			},
		})

		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
			Preferences: ui.Preferences{ReduceMotion: true},
		})
		if _, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.1 发布"); !ok {
			t.Fatal("remote announcement card missing")
		}
		if err := tt.Click("查看详情"); err != nil {
			t.Fatalf("click remote CTA: %v", err)
		}
		tt.Frame()
		if openedURL != "https://www.uniclipboard.app/blog/v2-1" {
			t.Fatalf("openedURL=%q want v2-1 blog URL", openedURL)
		}
		if len(dismissedIDs) != 1 || dismissedIDs[0] != "rel-2-1" {
			t.Fatalf("dismissedIDs=%v want [rel-2-1]", dismissedIDs)
		}
		if _, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.1 发布"); ok {
			t.Fatal("remote announcement remained visible after CTA click")
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

	t.Run("priority: critical remote beats local v2; local v2 beats routine info and hands off on dismiss", func(t *testing.T) {
		d := newDemo("history", "light")
		desktop := &Desktop{d: d}
		desktop.SetV2Announcement(true, nil)
		desktop.SetDismissedAnnouncements([]string{"already-dismissed"}, nil)
		desktop.SetRemoteAnnouncements([]announcements.Announcement{
			{ID: "already-dismissed", Severity: "critical", Title: "Ignored", Content: "x", PublishedAt: "2026-11-25T00:00:00Z"},
			{ID: "info-1", Severity: "info", Title: "Routine update", Content: "Details", ActionURL: "https://www.uniclipboard.app/blog/info", PublishedAt: "2026-11-25T00:00:00Z"},
		})

		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{
			Preferences: ui.Preferences{ReduceMotion: true},
		})
		// Local v2 beats routine info (and ignores already-dismissed critical).
		if _, ok := tt.AccessByRole(ui.RoleRegion, "UniClipboard 2.0 全新发布"); !ok {
			t.Fatal("expected local v2 announcement ahead of routine info")
		}
		// Dismiss local v2 -> queued routine info announcement appears next.
		if err := tt.Click("关闭"); err != nil {
			t.Fatalf("close local v2: %v", err)
		}
		tt.Frame()
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleRegion, "Routine update"); !ok {
			t.Fatal("queued remote info announcement did not appear after local v2 dismissed")
		}

		// Urgent critical remote beats local v2 when both are eligible initially.
		d2 := newDemo("history", "light")
		desktop2 := &Desktop{d: d2}
		desktop2.SetV2Announcement(true, nil)
		desktop2.SetRemoteAnnouncements([]announcements.Announcement{
			{ID: "crit-1", Severity: "critical", Title: "Urgent security fix", Content: "Upgrade now", PublishedAt: "2026-11-25T00:00:00Z"},
		})
		tt2 := ui.NewTesterWith(d2.view, 1100, 720, ui.TesterOptions{
			Preferences: ui.Preferences{ReduceMotion: true},
		})
		if _, ok := tt2.AccessByRole(ui.RoleRegion, "Urgent security fix"); !ok {
			t.Fatal("critical remote announcement should take priority over local v2")
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
