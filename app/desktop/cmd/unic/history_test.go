package main

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
)

// fakeDaemon serves a small in-memory history, newest first.
type fakeDaemon struct {
	api.Backend
	items   []api.HistoryItem
	deleted []string
	ran     string
}

func (f *fakeDaemon) find(id string) (int, error) {
	for i, it := range f.items {
		if it.ID == id {
			return i, nil
		}
	}
	return 0, &api.Error{Message: "not found", Status: 404}
}

func (f *fakeDaemon) ListHistory(_ context.Context, q api.HistoryQuery) (api.HistoryPage, error) {
	var out []api.HistoryItem
	for _, it := range f.items {
		if len(q.Kinds) > 0 && q.Kinds[0] != it.Kind {
			continue
		}
		out = append(out, it)
	}
	if q.Limit > 0 && len(out) > q.Limit {
		out = out[:q.Limit]
	}
	return api.HistoryPage{Items: out}, nil
}
func (f *fakeDaemon) GetHistory(_ context.Context, id string) (api.HistoryItem, error) {
	i, err := f.find(id)
	if err != nil {
		return api.HistoryItem{}, err
	}
	return f.items[i], nil
}
func (f *fakeDaemon) HistoryContent(_ context.Context, id string, part int) (api.HistoryContent, error) {
	if _, err := f.find(id); err != nil {
		return api.HistoryContent{}, err
	}
	s := "body of " + id
	return api.HistoryContent{MIME: "text/plain", Size: int64(len(s)), ReadCloser: io.NopCloser(strings.NewReader(s))}, nil
}
func (f *fakeDaemon) UpdateHistory(_ context.Context, id string, u api.HistoryUpdate) (api.HistoryItem, error) {
	i, err := f.find(id)
	if err != nil {
		return api.HistoryItem{}, err
	}
	if u.Revision != f.items[i].Revision {
		return api.HistoryItem{}, &api.Error{Message: "stale revision", Status: 409}
	}
	if u.Tags != nil {
		f.items[i].Tags = *u.Tags
	}
	if u.ExpiresAtMs != nil {
		f.items[i].ExpiresAt = *u.ExpiresAtMs
	}
	f.items[i].Revision++
	return f.items[i], nil
}
func (f *fakeDaemon) DeleteHistory(_ context.Context, id string, rev uint64) error {
	i, err := f.find(id)
	if err != nil {
		return err
	}
	if rev != f.items[i].Revision {
		return &api.Error{Message: "stale revision", Status: 409}
	}
	f.deleted = append(f.deleted, id)
	f.items = append(f.items[:i], f.items[i+1:]...)
	return nil
}
func (f *fakeDaemon) Actions(context.Context) ([]api.Action, error) {
	return []api.Action{{Key: "builtin.text/upper", Title: "Uppercase"}}, nil
}
func (f *fakeDaemon) RunAction(_ context.Context, id, key string) (api.HistoryItem, error) {
	f.ran = id + " " + key
	return api.HistoryItem{ID: "new"}, nil
}

func newFake(t *testing.T) (*fakeDaemon, *api.Client) {
	t.Helper()
	f := &fakeDaemon{}
	for i := 1; i <= 3; i++ {
		kind := "text"
		if i == 2 {
			kind = "image"
		}
		f.items = append(f.items, api.HistoryItem{ID: fmt.Sprintf("h%d", i), Revision: 1, Kind: kind, Origin: "local_system",
			CreatedAt: 1700000000000, Parts: []api.HistoryPart{{MIME: map[string]string{"text": "text/plain", "image": "image/png"}[kind]}}})
	}
	dir, err := os.MkdirTemp("", "unich")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	sock := filepath.Join(dir, "s")
	ln, err := api.Listen(sock)
	if err != nil {
		t.Fatal(err)
	}
	srv := &http.Server{Handler: api.Handler(f, nil)}
	go func() { _ = srv.Serve(ln) }()
	t.Cleanup(func() { _ = srv.Close() })
	return f, api.NewClient(sock)
}

func runH(t *testing.T, c *api.Client, in string, tty bool, args ...string) (stdout, stderr string, err error) {
	t.Helper()
	var o, e bytes.Buffer
	err = historyCmd(context.Background(), c, args, historyIO{in: strings.NewReader(in), out: &o, err: &e, tty: tty})
	return o.String(), e.String(), err
}

func TestHistoryListShowsRefsAndFilters(t *testing.T) {
	_, c := newFake(t)
	out, _, err := runH(t, c, "", false, "list")
	if err != nil || !strings.Contains(out, "@1") || !strings.Contains(out, "h3") || strings.Contains(out, "body of") {
		t.Fatalf("%q %v", out, err)
	}
	out, _, _ = runH(t, c, "", false, "list", "--type", "image", "--json")
	if !strings.Contains(out, `"h2"`) || strings.Contains(out, `"h1"`) {
		t.Fatalf("filter: %q", out)
	}
}

func TestHistoryShowByRefAndDefault(t *testing.T) {
	_, c := newFake(t)
	for _, tc := range []struct {
		args []string
		want string
	}{
		{[]string{"show"}, "body of h1"},
		{[]string{"show", "@3"}, "body of h3"},
		{[]string{"show", "h2", "--part", "0"}, "body of h2"},
	} {
		out, _, err := runH(t, c, "", false, tc.args...)
		if err != nil || out != tc.want {
			t.Fatalf("%v: %q %v", tc.args, out, err)
		}
	}
	if _, _, err := runH(t, c, "", false, "show", "@9"); err == nil {
		t.Fatal("@9 should fail")
	}
	if _, _, err := runH(t, c, "", false, "show", "h2"); err == nil || !strings.Contains(err.Error(), "no text part") {
		t.Fatalf("image without --part: %v", err)
	}
}

func TestHistoryDeleteConfirmation(t *testing.T) {
	f, c := newFake(t)
	// A terminal is asked; "n" cancels.
	if _, _, err := runH(t, c, "n\n", true, "delete", "@1"); err != nil || len(f.deleted) != 0 {
		t.Fatalf("cancel: %v %v", err, f.deleted)
	}
	// "y" deletes.
	if _, _, err := runH(t, c, "y\n", true, "delete", "@1"); err != nil || len(f.deleted) != 1 {
		t.Fatalf("confirm: %v %v", err, f.deleted)
	}
	// -y skips the question; a pipe is never asked.
	if _, _, err := runH(t, c, "", true, "delete", "-y", "h2"); err != nil {
		t.Fatal(err)
	}
	if _, _, err := runH(t, c, "", false, "delete", "h3"); err != nil || len(f.items) != 0 {
		t.Fatalf("pipe: %v %v", err, f.items)
	}
}

func TestHistoryTagExpireAction(t *testing.T) {
	f, c := newFake(t)
	if _, _, err := runH(t, c, "", false, "tag", "@1", "--add", "a,b"); err != nil {
		t.Fatal(err)
	}
	if _, _, err := runH(t, c, "", false, "tag", "@1", "--add", "c", "--remove", "a"); err != nil {
		t.Fatal(err)
	}
	if got := strings.Join(f.items[0].Tags, ","); got != "b,c" {
		t.Fatalf("tags %q", got)
	}
	if _, _, err := runH(t, c, "", false, "tag", "@1", "--set", ""); err != nil || len(f.items[0].Tags) != 0 {
		t.Fatalf("clear: %v %v", err, f.items[0].Tags)
	}
	if _, _, err := runH(t, c, "", false, "expire", "@1", "7d"); err != nil || f.items[0].ExpiresAt == 0 {
		t.Fatalf("expire: %v", err)
	}
	if _, _, err := runH(t, c, "", false, "expire", "@1", "never"); err != nil || f.items[0].ExpiresAt != 0 {
		t.Fatalf("never: %v", err)
	}
	if _, _, err := runH(t, c, "", false, "expire", "@1", "soon"); err == nil {
		t.Fatal("bad duration accepted")
	}
	if out, _, err := runH(t, c, "", false, "action", "@1"); err != nil || !strings.Contains(out, "builtin.text/upper") {
		t.Fatalf("%q %v", out, err)
	}
	if _, _, err := runH(t, c, "", false, "action", "@2", "builtin.text/upper"); err != nil || f.ran != "h2 builtin.text/upper" {
		t.Fatalf("ran %q %v", f.ran, err)
	}
}
