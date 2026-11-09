package host

import (
	"slices"
	"testing"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func TestBuiltinTextTagsCopiesAndRunsActions(t *testing.T) {
	h := newHost(t, false)
	settle(h)
	h.board.user(text("mail me@example.com"))
	eventually(t, "recorded", func() bool { return len(historyIDs(t, h)) == 1 })
	src := historyIDs(t, h)[0]
	if !slices.Contains(src.Tags, "email") {
		t.Fatalf("tags = %v", src.Tags)
	}

	acts, err := h.n.Actions(bg)
	if err != nil || len(acts) == 0 {
		t.Fatalf("actions = %v, %v", acts, err)
	}
	out, err := h.n.ExecuteAction(bg, src.ID, "builtin.text/upper", api.ActionInput{Apply: true})
	it := out.Item
	if err != nil {
		t.Fatal(err)
	}
	if it.ID == src.ID || it.Origin != "action" || it.PluginID != "builtin.text" {
		t.Fatalf("result = %+v", it)
	}
	if got(h) != "MAIL ME@EXAMPLE.COM" {
		t.Fatalf("clipboard = %q", got(h))
	}
	eventually(t, "two records", func() bool { return len(historyIDs(t, h)) == 2 })
	for _, x := range historyIDs(t, h) {
		if x.ID == src.ID && x.Revision != src.Revision {
			t.Fatal("the source record changed")
		}
	}
}
