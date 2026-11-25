package setupflow

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"testing"
)

func TestV2AnnouncementLifecycle(t *testing.T) {
	t.Run("fresh install enables announcement across setup completion until dismissed", func(t *testing.T) {
		dir := t.TempDir()
		flow, err := Open(dir)
		if err != nil {
			t.Fatal(err)
		}
		if !flow.Required() {
			t.Fatal("fresh install should require setup")
		}
		if !flow.State().V2Announcement {
			t.Fatal("fresh install should enable v2 announcement")
		}

		if err := flow.Complete(); err != nil {
			t.Fatal(err)
		}
		reopened, err := Open(dir)
		if err != nil {
			t.Fatal(err)
		}
		if reopened.Required() {
			t.Fatal("completed setup should not require setup")
		}
		if !reopened.State().V2Announcement {
			t.Fatal("completing first-install setup should keep v2 announcement visible")
		}

		if err := reopened.DismissV2Announcement(); err != nil {
			t.Fatal(err)
		}
		afterDismiss, err := Open(dir)
		if err != nil {
			t.Fatal(err)
		}
		if afterDismiss.State().V2Announcement {
			t.Fatal("dismissing v2 announcement did not persist")
		}
		// Idempotent second dismiss.
		if err := afterDismiss.DismissV2Announcement(); err != nil {
			t.Fatal(err)
		}
		// Later migration from Settings must not re-enable the dismissed announcement.
		if err := afterDismiss.BeginMigration(); err != nil {
			t.Fatal(err)
		}
		if err := afterDismiss.Complete(); err != nil {
			t.Fatal(err)
		}
		finalFlow, err := Open(dir)
		if err != nil {
			t.Fatal(err)
		}
		if finalFlow.State().V2Announcement {
			t.Fatal("later migration re-enabled dismissed v2 announcement")
		}
	})

	t.Run("established v2 database does not show first-install announcement", func(t *testing.T) {
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "unic.db"), []byte("sqlite-bytes"), 0600); err != nil {
			t.Fatal(err)
		}
		flow, err := Open(dir)
		if err != nil {
			t.Fatal(err)
		}
		if flow.Required() {
			t.Fatal("established v2 database should bypass setup")
		}
		if flow.State().V2Announcement {
			t.Fatal("established v2 database should not show first-install announcement")
		}
	})

	t.Run("existing completed desktop-setup.json does not show announcement", func(t *testing.T) {
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "desktop-setup.json"), []byte(`{"complete":true}`), 0600); err != nil {
			t.Fatal(err)
		}
		flow, err := Open(dir)
		if err != nil {
			t.Fatal(err)
		}
		if flow.State().V2Announcement {
			t.Fatal("existing completed profile should not show first-install announcement")
		}
	})

	t.Run("dismissed FlareRelease announcement IDs persist, deduplicate and stay bounded", func(t *testing.T) {
		dir := t.TempDir()
		flow, err := Open(dir)
		if err != nil {
			t.Fatal(err)
		}
		if err := flow.DismissAnnouncement("  "); err != nil {
			t.Fatal(err)
		}
		if err := flow.DismissAnnouncement("notice-1"); err != nil {
			t.Fatal(err)
		}
		if err := flow.DismissAnnouncement("notice-1"); err != nil {
			t.Fatal(err)
		}
		if err := flow.DismissAnnouncement("notice-2"); err != nil {
			t.Fatal(err)
		}
		reopened, err := Open(dir)
		if err != nil {
			t.Fatal(err)
		}
		if !slices.Equal(reopened.State().DismissedAnnouncements, []string{"notice-1", "notice-2"}) {
			t.Fatalf("dismissed=%v want [notice-1 notice-2]", reopened.State().DismissedAnnouncements)
		}
		for i := 0; i < maxDismissedAnnouncements+25; i++ {
			if err := reopened.DismissAnnouncement(fmt.Sprintf("bulk-%d", i)); err != nil {
				t.Fatal(err)
			}
		}
		got := reopened.State().DismissedAnnouncements
		if len(got) != maxDismissedAnnouncements {
			t.Fatalf("len=%d want %d", len(got), maxDismissedAnnouncements)
		}
		if got[len(got)-1] != fmt.Sprintf("bulk-%d", maxDismissedAnnouncements+24) {
			t.Fatalf("tail=%q", got[len(got)-1])
		}
	})
}
