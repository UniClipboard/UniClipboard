package desktopui

import (
	"context"
	"encoding/json"
	"image/png"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"
)

func TestValidHTTPURL(t *testing.T) {
	for _, good := range []string{
		"https://wss.ink/f/abc123",
		"  http://example.com/path?q=1#frag  ",
	} {
		if got := validHTTPURL(good); got == "" {
			t.Errorf("validHTTPURL(%q) = empty, want valid", good)
		}
	}
	for _, bad := range []string{
		"",
		"file:///etc/passwd",
		"javascript:alert(1)",
		"https://example.com/a\nhttps://evil.com",
		"https://example.com/with space",
		"not a url",
	} {
		if got := validHTTPURL(bad); got != "" {
			t.Errorf("validHTTPURL(%q) = %q, want empty", bad, got)
		}
	}
}

func TestHistoryActionPostActionsDesktop(t *testing.T) {
	dir, err := os.MkdirTemp("", "unic-post-action-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = os.RemoveAll(dir) }()
	socket := filepath.Join(dir, "s")
	ln, err := net.Listen("unix", socket)
	if err != nil {
		t.Fatal(err)
	}

	type actionReq struct {
		path string
		in   api.ActionInput
	}
	reqs := make(chan actionReq, 8)
	restores := make(chan string, 4)

	srv := &http.Server{Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.Method == http.MethodPost && strings.HasPrefix(r.URL.Path, "/v1/history/") && strings.Contains(r.URL.Path, "/actions/"):
			var in api.ActionInput
			_ = json.NewDecoder(r.Body).Decode(&in)
			reqs <- actionReq{path: r.URL.Path, in: in}
			w.Header().Set("Content-Type", "application/json")
			itemID := "new-share"
			if strings.HasSuffix(r.URL.Path, "/none") {
				itemID = "new-none"
			} else if strings.HasSuffix(r.URL.Path, "/zh") {
				itemID = "new-present"
			}
			_ = json.NewEncoder(w).Encode(api.ActionOutput{
				Item:    api.HistoryItem{ID: itemID, Kind: "text"},
				Applied: in.Apply,
			})
		case r.Method == http.MethodGet && strings.HasPrefix(r.URL.Path, "/v1/history/") && strings.HasSuffix(r.URL.Path, "/content"):
			w.Header().Set("Content-Type", "text/plain; charset=utf-8")
			if strings.Contains(r.URL.Path, "new-share") {
				_, _ = w.Write([]byte("https://wss.ink/f/demo2026share"))
			} else {
				_, _ = w.Write([]byte("翻译结果：剪贴板跨设备同步"))
			}
		case r.Method == http.MethodPost && strings.HasPrefix(r.URL.Path, "/v1/history/") && strings.HasSuffix(r.URL.Path, "/use"):
			restores <- r.URL.Path
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{}`))
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	})}
	defer func() { _ = srv.Close() }()
	go func() { _ = srv.Serve(ln) }()

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	live := &liveState{
		ctx:         ctx,
		client:      api.NewClient(socket),
		jobs:        make(chan func(context.Context) func(), 4),
		results:     make(chan func(), 4),
		details:     map[string]entry{},
		detailReads: map[string]*detailRead{},
		items: map[string]api.HistoryItem{
			"code": {ID: "code", Kind: "text"},
		},
		actions: []api.Action{
			{
				Key:         "unic.wenshushu/share",
				PluginID:    "unic.wenshushu",
				Title:       "通过文叔叔临时分享",
				Accepts:     []string{"text", "image", "files"},
				Placements:  []string{plugin.ActionPlacementHistoryItemMenu, plugin.ActionPlacementHistoryToolbar},
				PostActions: []string{plugin.ActionPostCopy, plugin.ActionPostPresent},
			},
			{
				Key:         "example.translate/zh",
				PluginID:    "example.translate",
				Title:       "翻译为中文",
				Accepts:     []string{"text"},
				Placements:  []string{plugin.ActionPlacementHistoryItemMenu},
				PostActions: []string{plugin.ActionPostPresent},
			},
			{
				Key:         "example.archive/none",
				PluginID:    "example.archive",
				Title:       "仅归档",
				Accepts:     []string{"text"},
				Placements:  []string{plugin.ActionPlacementHistoryItemMenu},
				PostActions: []string{plugin.ActionPostNone},
			},
		},
	}
	go live.work()

	drain := func(t *testing.T, d *viewState) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for d.live.busy && time.Now().Before(deadline) {
			select {
			case apply := <-d.live.results:
				apply()
			case <-time.After(10 * time.Millisecond):
			}
		}
		if d.live.busy {
			t.Fatal("worker did not finish")
		}
	}

	// 1. copy + present: passes Apply=true, opens modal with URL + copied badge + Open Link + Re-copy.
	d := newDemo("history", "dark")
	d.layout = "grid"
	live.key = d.liveQueryKey()
	live.nextRefresh = time.Now().Add(time.Hour)
	live.catalogueKey = `["","",0]`
	d.live = live
	var openedURL string
	d.openURL = func(u string) error { openedURL = u; return nil }

	d.runHistoryAction("code", "unic.wenshushu/share")
	req := <-reqs
	if !req.in.Apply || !req.in.CancelOnContext {
		t.Fatalf("share action request = %+v, want Apply=true CancelOnContext=true", req.in)
	}
	drain(t, d)
	if !d.actionResult.open || !d.actionResult.copied || d.actionResult.url != "https://wss.ink/f/demo2026share" {
		t.Fatalf("unexpected share actionResult: %+v", d.actionResult)
	}
	live.nextRefresh = time.Now().Add(time.Hour)
	live.message = ""
	live.readError = ""

	tt := ui.NewTesterWith(d.view, 1180, 760, ui.TesterOptions{Dark: true})
	tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
	tt.Advance(300 * time.Millisecond)
	if snapDir := os.Getenv("UNIC_GRID_SNAP_DIR"); snapDir != "" {
		tt.SetScale(2)
		tt.Advance(50 * time.Millisecond)
		if err := os.MkdirAll(snapDir, 0755); err == nil {
			if f, err := os.Create(filepath.Join(snapDir, "action-result-present-dark.png")); err == nil {
				_ = png.Encode(f, tt.Image())
				_ = f.Close()
			}
		}
	}
	for _, want := range []string{"通过文叔叔临时分享", "unic.wenshushu", "已自动复制到剪贴板", "https://wss.ink/f/demo2026share", "打开链接", "再次复制"} {
		if !tt.HasText(want) {
			t.Fatalf("present dialog missing %q: %v", want, tt.Texts())
		}
	}
	if err := tt.Click("打开链接"); err != nil {
		t.Fatalf("click open link: %v", err)
	}
	if openedURL != "https://wss.ink/f/demo2026share" {
		t.Fatalf("openedURL = %q", openedURL)
	}
	if err := tt.Click("再次复制"); err != nil {
		t.Fatalf("click recopy: %v", err)
	}
	select {
	case p := <-restores:
		if p != "/v1/history/new-share/use" {
			t.Fatalf("unexpected restore path: %s", p)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("restore endpoint not called")
	}
	drain(t, d)
	if err := tt.Click("关闭"); err != nil {
		t.Fatalf("click close: %v", err)
	}
	if d.actionResult.open {
		t.Fatal("present dialog stayed open after close")
	}

	// 2. present-only (no copy): passes Apply=false, opens modal without copied badge and without Open Link for non-URL text.
	d.runHistoryAction("code", "example.translate/zh")
	req = <-reqs
	if req.in.Apply {
		t.Fatalf("present-only action unexpectedly set Apply=true")
	}
	drain(t, d)
	if !d.actionResult.open || d.actionResult.copied || d.actionResult.url != "" || d.actionResult.text != "翻译结果：剪贴板跨设备同步" {
		t.Fatalf("unexpected present-only state: %+v", d.actionResult)
	}
	d.actionResult = actionResultState{}

	// 3. none: passes Apply=false, shows neither dialog nor toast.
	d.runHistoryAction("code", "example.archive/none")
	req = <-reqs
	if req.in.Apply {
		t.Fatalf("none post_action unexpectedly set Apply=true")
	}
	drain(t, d)
	if d.actionResult.open || d.actionToast.visible || d.notice != "" {
		t.Fatalf("none post_action left UI side effects: result=%+v toast=%+v notice=%q", d.actionResult, d.actionToast, d.notice)
	}
}
