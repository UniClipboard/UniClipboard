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

func TestQRScannableURL(t *testing.T) {
	exactMax := "https://example.com/" + strings.Repeat("a", maxActionQRURLBytes-len("https://example.com/"))
	for _, good := range []string{
		"https://wss.ink/f/demo2026share",
		"https://wormhole.app/demo123#k9vX_secretKeyFragment",
		"  http://192.168.1.20:8080/dl/file.zip  ",
		exactMax,
	} {
		if got := qrScannableURL(good); got == "" {
			t.Errorf("qrScannableURL(%q) = empty, want scannable", good)
		}
	}
	tooLong := exactMax + "x"
	for _, bad := range []string{
		"",
		"not a url",
		"javascript:alert(1)",
		"file:///etc/passwd",
		"http://localhost:8080/share",
		"https://app.localhost/share",
		"http://127.0.0.1:8080/share",
		"http://[::1]:8080/share",
		"http://0.0.0.0:8080/share",
		tooLong,
	} {
		if got := qrScannableURL(bad); got != "" {
			t.Errorf("qrScannableURL(%q) = %q, want empty", bad, got)
		}
	}
}

func TestEncodeActionQRImage(t *testing.T) {
	longScannable := "https://example.com/dl/" + strings.Repeat("k", 650) + "#frag"
	for _, raw := range []string{
		"https://wss.ink/f/demo2026share",
		"https://wormhole.app/demo123#secretKeyFragment",
		longScannable,
	} {
		img := encodeActionQRImage(raw)
		if img == nil {
			t.Fatalf("encodeActionQRImage(%q) = nil, want QR image", raw)
		}
		b := img.Bounds()
		if b.Dx() != b.Dy() || b.Dx() < actionQRMinPixels {
			t.Fatalf("encodeActionQRImage(%q) bounds = %v, want square >= %d", raw, b, actionQRMinPixels)
		}
		// Standard four-module quiet zone keeps all four corners pure white.
		for _, pt := range [][2]int{{0, 0}, {b.Dx() - 1, 0}, {0, b.Dy() - 1}, {b.Dx() - 1, b.Dy() - 1}} {
			c := img.NRGBAAt(pt[0], pt[1])
			if c.R != 255 || c.G != 255 || c.B != 255 || c.A != 255 {
				t.Fatalf("quiet zone corner %v = %+v, want pure white", pt, c)
			}
		}
		hasBlack := false
		for i := 0; i < len(img.Pix); i += 4 {
			if img.Pix[i] == 0 && img.Pix[i+1] == 0 && img.Pix[i+2] == 0 && img.Pix[i+3] == 255 {
				hasBlack = true
				break
			}
		}
		if !hasBlack {
			t.Fatalf("encodeActionQRImage(%q) has no dark modules", raw)
		}
	}
	if img := encodeActionQRImage("http://127.0.0.1:8080/local"); img != nil {
		t.Fatal("loopback URL unexpectedly produced a QR image")
	}
	if img := encodeActionQRImage("plain text result"); img != nil {
		t.Fatal("non-URL text unexpectedly produced a QR image")
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
			switch {
			case strings.HasSuffix(r.URL.Path, "/none"):
				itemID = "new-none"
			case strings.HasSuffix(r.URL.Path, "/zh"):
				itemID = "new-present"
			case strings.Contains(r.URL.Path, "unic.wormhole"):
				itemID = "new-wormhole"
			case strings.HasSuffix(r.URL.Path, "/local"):
				itemID = "new-local"
			}
			_ = json.NewEncoder(w).Encode(api.ActionOutput{
				Item:    api.HistoryItem{ID: itemID, Kind: "text"},
				Applied: in.Apply,
			})
		case r.Method == http.MethodGet && strings.HasPrefix(r.URL.Path, "/v1/history/") && strings.HasSuffix(r.URL.Path, "/content"):
			w.Header().Set("Content-Type", "text/plain; charset=utf-8")
			switch {
			case strings.Contains(r.URL.Path, "new-share"):
				_, _ = w.Write([]byte("https://wss.ink/f/demo2026share"))
			case strings.Contains(r.URL.Path, "new-wormhole"):
				_, _ = w.Write([]byte("https://wormhole.app/demo123#secretKeyFragment"))
			case strings.Contains(r.URL.Path, "new-local"):
				_, _ = w.Write([]byte("http://127.0.0.1:8080/preview"))
			default:
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
				Key:         "unic.wormhole/share",
				PluginID:    "unic.wormhole",
				Title:       "通过 Wormhole 临时分享",
				Accepts:     []string{"text", "image", "files"},
				Placements:  []string{plugin.ActionPlacementHistoryItemMenu, plugin.ActionPlacementHistoryToolbar},
				PostActions: []string{plugin.ActionPostCopy, plugin.ActionPostPresent},
			},
			{
				Key:         "example.preview/local",
				PluginID:    "example.preview",
				Title:       "本地预览链接",
				Accepts:     []string{"text"},
				Placements:  []string{plugin.ActionPlacementHistoryItemMenu},
				PostActions: []string{plugin.ActionPostPresent},
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
	if !d.actionResult.open || !d.actionResult.copied || d.actionResult.url != "https://wss.ink/f/demo2026share" || d.actionResult.qr == nil {
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
	for _, want := range []string{"通过文叔叔临时分享", "unic.wenshushu", "已自动复制到剪贴板", "https://wss.ink/f/demo2026share", "使用手机扫码打开链接", "打开链接", "再次复制"} {
		if !tt.HasText(want) {
			t.Fatalf("present dialog missing %q: %v", want, tt.Texts())
		}
	}
	if qrNode, ok := tt.AccessByRole(ui.RoleImage, "链接二维码"); !ok || qrNode.Bounds.W < 150 || qrNode.Bounds.H < 150 {
		t.Fatalf("present dialog missing QR image: ok=%v node=%+v", ok, qrNode)
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
	if d.actionResult.open || d.actionResult.qr != nil {
		t.Fatal("present dialog stayed open or retained QR bitmap after close")
	}

	// 1b. Wormhole URL with #fragment keeps the full link and shows QR code by default.
	d.runHistoryAction("code", "unic.wormhole/share")
	req = <-reqs
	if !req.in.Apply || !req.in.CancelOnContext {
		t.Fatalf("wormhole action request = %+v, want Apply=true CancelOnContext=true", req.in)
	}
	drain(t, d)
	if !d.actionResult.open || d.actionResult.url != "https://wormhole.app/demo123#secretKeyFragment" || d.actionResult.qr == nil {
		t.Fatalf("unexpected wormhole actionResult: %+v", d.actionResult)
	}
	live.nextRefresh = time.Now().Add(time.Hour)
	live.message = ""
	live.readError = ""
	d.theme = "light"
	ttLight := ui.NewTesterWith(d.view, 1180, 760, ui.TesterOptions{Dark: false})
	ttLight.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
	ttLight.Advance(300 * time.Millisecond)
	if _, ok := ttLight.AccessByRole(ui.RoleImage, "链接二维码"); !ok {
		t.Fatal("wormhole present dialog missing QR image")
	}
	if !ttLight.HasText("https://wormhole.app/demo123#secretKeyFragment") {
		t.Fatalf("wormhole present dialog missing fragment URL: %v", ttLight.Texts())
	}
	if snapDir := os.Getenv("UNIC_GRID_SNAP_DIR"); snapDir != "" {
		ttLight.SetScale(2)
		ttLight.Advance(50 * time.Millisecond)
		if err := os.MkdirAll(snapDir, 0755); err == nil {
			if f, err := os.Create(filepath.Join(snapDir, "action-result-present-light.png")); err == nil {
				_ = png.Encode(f, ttLight.Image())
				_ = f.Close()
			}
		}
	}
	d.theme = "dark"
	d.actionResult = actionResultState{}

	// 1c. Loopback URL keeps Open Link button but omits the phone QR code.
	d.runHistoryAction("code", "example.preview/local")
	<-reqs
	drain(t, d)
	if !d.actionResult.open || d.actionResult.url != "http://127.0.0.1:8080/preview" || d.actionResult.qr != nil {
		t.Fatalf("unexpected loopback actionResult: %+v", d.actionResult)
	}
	live.nextRefresh = time.Now().Add(time.Hour)
	live.message = ""
	live.readError = ""
	ttLocal := ui.NewTesterWith(d.view, 1180, 760, ui.TesterOptions{Dark: true})
	ttLocal.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
	ttLocal.Advance(300 * time.Millisecond)
	if ttLocal.HasText("使用手机扫码打开链接") {
		t.Fatal("loopback URL unexpectedly rendered QR caption")
	}
	if _, ok := ttLocal.AccessByRole(ui.RoleImage, "链接二维码"); ok {
		t.Fatal("loopback URL unexpectedly rendered QR image")
	}
	if !ttLocal.HasText("打开链接") {
		t.Fatalf("loopback URL missing Open Link button: %v", ttLocal.Texts())
	}
	d.actionResult = actionResultState{}

	// 2. present-only (no copy): passes Apply=false, opens modal without copied badge, QR code, or Open Link for non-URL text.
	d.runHistoryAction("code", "example.translate/zh")
	req = <-reqs
	if req.in.Apply {
		t.Fatalf("present-only action unexpectedly set Apply=true")
	}
	drain(t, d)
	if !d.actionResult.open || d.actionResult.copied || d.actionResult.url != "" || d.actionResult.qr != nil || d.actionResult.text != "翻译结果：剪贴板跨设备同步" {
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
	if d.actionResult.open || d.toasts.Len() > 0 || d.notice != "" {
		t.Fatalf("none post_action left UI side effects: result=%+v toasts=%d notice=%q", d.actionResult, d.toasts.Len(), d.notice)
	}
}
