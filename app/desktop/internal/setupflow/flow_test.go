package setupflow

import (
	"fmt"
	"slices"
	"testing"
)

func TestDismissedAnnouncementsPersistence(t *testing.T) {
	dir := t.TempDir()
	flow, err := Open(dir)
	if err != nil {
		t.Fatal(err)
	}
	if err := flow.Complete(); err != nil {
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
	if reopened.Required() {
		t.Fatal("dismissing announcements lost setup completion")
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
}
