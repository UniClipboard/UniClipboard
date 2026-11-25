package desktopui

import (
	"context"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/plugins/builtinpreview"
)

func newSmartPreviewTestDesktop(t *testing.T) (*Desktop, context.CancelFunc) {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	d := &viewState{
		page:       "settings",
		theme:      "light",
		layout:     "list",
		settings:   desktopkit.NavState{Value: "plugins"},
		deviceList: desktopkit.SourceState{Value: "local"},
		options:    map[string]bool{},
		choices:    map[string]string{},
	}
	d.live = &liveState{
		ctx:               ctx,
		cancel:            cancel,
		client:            api.NewClient(filepath.Join(t.TempDir(), "unused.sock")),
		loaded:            true,
		loading:           true, // suppress background refresh in unit test
		nextRefresh:       time.Now().Add(time.Hour),
		results:           make(chan func(), 32),
		jobs:              make(chan func(context.Context) func(), 16),
		items:             map[string]api.HistoryItem{},
		details:           map[string]entry{},
		pending:           map[string]api.PendingPeer{},
		config:            config.State{Revision: "rev-1", Config: config.Default()},
		marketplaceLoaded: true,
	}
	return &Desktop{d: d}, cancel
}

func TestSmartPreviewSettingsListAndDetailRender(t *testing.T) {
	desk, cancel := newSmartPreviewTestDesktop(t)
	defer cancel()

	// Built-in list displays Smart Preview in both zh-CN and en locales.
	tt := ui.NewTester(desk.View, 1100, 760)
	if !tt.HasText("智能预览") {
		t.Fatalf("expected Smart Preview entry in zh-CN built-in plugin list; texts=%v", tt.Texts())
	}

	desk.d.live.config.Config.Desktop.Language = "en"
	ttEn := ui.NewTester(desk.View, 1100, 760)
	if !ttEn.HasText("Smart Preview") {
		t.Fatalf("expected Smart Preview entry in en built-in plugin list; texts=%v", ttEn.Texts())
	}

	// Open Smart Preview detail page in zh-CN and verify dedicated controls without processor on_error selector.
	desk.d.live.config.Config.Desktop.Language = "zh-CN"
	desk.d.pluginSettings.id = builtinpreview.ID
	ttDetail := ui.NewTester(desk.View, 1100, 760)
	for _, want := range []string{"智能预览", "启用智能预览", "网页链接预览", "抓取网站图标", "抓取网页封面图", "排除域名"} {
		if !ttDetail.HasText(want) {
			t.Fatalf("expected %q on Smart Preview detail page; texts=%v", want, ttDetail.Texts())
		}
	}
	if ttDetail.HasText("处理失败时") {
		t.Fatal("Smart Preview enricher detail must not render processor on_error selector")
	}
}

func TestParseExcludedDomainsDraft(t *testing.T) {
	got := parseExcludedDomainsDraft(" intranet.example , corp.local，, internal.test ")
	want := []string{"intranet.example", "corp.local", "internal.test"}
	if !slices.Equal(got, want) {
		t.Fatalf("got %v, want %v", got, want)
	}
}
