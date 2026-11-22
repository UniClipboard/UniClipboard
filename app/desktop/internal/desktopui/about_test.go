package desktopui

import (
	"bytes"
	"context"
	"errors"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopui/preview"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestSponsorInitials(t *testing.T) {
	cases := []struct {
		name string
		want string
	}{
		{"Ada Lovelace", "AL"},
		{"Linus", "LI"},
		{"X", "X"},
		{"云端用户", "云"},
		{"  ", "?"},
		{"", "?"},
	}
	for _, tc := range cases {
		if got := sponsorInitials(tc.name); got != tc.want {
			t.Errorf("sponsorInitials(%q) = %q, want %q", tc.name, got, tc.want)
		}
	}
}

func TestParseAboutChannel(t *testing.T) {
	cases := []struct {
		ver, cfg, want string
	}{
		{"2.0.0", "stable", "stable"},
		{"2.0.0", "alpha", "alpha"},
		{"0.4.0-alpha.6", "stable", "alpha"},
		{"0.4.0-beta.1", "stable", "beta"},
		{"0.4.0-rc.2", "stable", "rc"},
		{"dev", "stable", "dev"},
		{"dev", "alpha", "alpha"},
	}
	for _, tc := range cases {
		if got := parseAboutChannel(tc.ver, tc.cfg); got != tc.want {
			t.Errorf("parseAboutChannel(%q, %q) = %q, want %q", tc.ver, tc.cfg, got, tc.want)
		}
	}
}

func newTestAboutLive(t *testing.T) (*Desktop, *viewState) {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	d := &viewState{
		page:       "settings",
		theme:      "light",
		layout:     "list",
		nav:        desktopkit.NavState{Value: "all"},
		settings:   desktopkit.NavState{Value: "about"},
		deviceList: desktopkit.SourceState{Value: "local"},
		gridSize:   .4,
		options:    map[string]bool{},
		choices:    map[string]string{},
		about: aboutState{
			version:     "2.0.0",
			buildNumber: "1",
		},
	}
	cfg := config.Default()
	d.live = &liveState{
		ctx:         ctx,
		cancel:      cancel,
		client:      api.NewClient(filepath.Join(t.TempDir(), "unused.sock")),
		jobs:        make(chan func(context.Context) func(), 16),
		results:     make(chan func(), 32),
		changed:     make(chan struct{}, 1),
		recovery:    make(chan struct{}, 1),
		searchSlots: make(chan struct{}, 1),
		loaded:      true,
		connected:   true,
		key:         d.liveQueryKey(),
		nextRefresh: time.Now().Add(time.Hour),
		config:      config.State{Config: cfg},
	}
	return &Desktop{d: d}, d
}

func TestAboutHeaderAndLinks(t *testing.T) {
	desktop := NewPreview(preview.Desktop{Page: "settings", Theme: "light", Layout: "list"})
	d := desktop.d
	d.settings.Value = "about"
	var opened []string
	desktop.SetOpenExternal(func(u string) error {
		opened = append(opened, u)
		return nil
	})

	tt := ui.NewTesterWith(d.view, 1100, 900, ui.TesterOptions{})
	if dir := os.Getenv("UNIC_ABOUT_CAPTURE_DIR"); dir != "" {
		savePNG := func(name string, tester *ui.Tester) {
			f, err := os.Create(filepath.Join(dir, name))
			if err != nil {
				t.Fatal(err)
			}
			if err := png.Encode(f, tester.Image()); err != nil {
				t.Fatal(err)
			}
			_ = f.Close()
		}
		savePNG("about-light.png", tt)
		darkDesktop := NewPreview(preview.Desktop{Page: "settings", Theme: "dark", Layout: "list"})
		darkDesktop.d.settings.Value = "about"
		darkTT := ui.NewTesterWith(darkDesktop.d.view, 1100, 900, ui.TesterOptions{Dark: true})
		savePNG("about-dark.png", darkTT)
	}
	for _, want := range []string{
		"UniClipboard",
		"Stable",
		"版本 2.0.0 (1)",
		"检查更新",
		"更新",
		"自动检查更新",
		"后台下载更新",
		"更新渠道",
		"赞助者",
		"Ada Lovelace",
		"金牌",
		"成为赞助者 →",
		"去 GitHub 点 Star →",
		"隐私政策",
		"使用条款",
		"关于与维护者",
		"© 2025 UniClipboard. All rights reserved.",
	} {
		if !tt.HasText(want) {
			t.Fatalf("missing expected About text %q", want)
		}
	}

	// Click footer and sponsor links.
	for _, tc := range []struct {
		label string
		url   string
	}{
		{"成为赞助者 →", sponsorChannelURL},
		{"去 GitHub 点 Star →", githubRepositoryURL},
		{"隐私政策", privacyPolicyURL},
		{"使用条款", termsOfServiceURL},
		{"关于与维护者", aboutMaintainersURL},
	} {
		if err := tt.Click(tc.label); err != nil {
			t.Fatalf("click %q: %v", tc.label, err)
		}
		if len(opened) == 0 || opened[len(opened)-1] != tc.url {
			t.Fatalf("click %q opened %v, want %q", tc.label, opened, tc.url)
		}
	}
}

func TestAboutUpdatePreferencesAndAlphaConfirmation(t *testing.T) {
	desktop, d := newTestAboutLive(t)
	d.about.sponsorsStarted = true
	d.about.sponsorsLoaded = true // hide sponsors to keep focus on update controls

	status := UpdateStatus{
		Supported:          true,
		Owner:              "direct",
		AutoCheckUpdate:    true,
		AutoDownloadUpdate: false,
	}
	checkCalls := 0
	var lastAutoCheck, lastAutoDownload *bool
	desktop.SetUpdater(
		func() UpdateStatus { return status },
		func() { checkCalls++ },
		func(ac, ad *bool) {
			if ac != nil {
				v := *ac
				lastAutoCheck = &v
				status.AutoCheckUpdate = v
			}
			if ad != nil {
				v := *ad
				lastAutoDownload = &v
				status.AutoDownloadUpdate = v
			}
		},
	)

	tt := ui.NewTesterWith(d.view, 1100, 900, ui.TesterOptions{})

	// 1. Clicking Check for updates invokes updater check.
	if err := tt.Click("检查更新"); err != nil {
		t.Fatal(err)
	}
	if checkCalls != 1 {
		t.Fatalf("checkCalls = %d, want 1", checkCalls)
	}

	// 2. Toggle background download on while auto-check is enabled.
	dlSwitch, ok := tt.AccessByRole(ui.RoleSwitch, "后台下载更新")
	if !ok {
		t.Fatal("auto-download switch missing")
	}
	tt.Press(dlSwitch.Bounds.X+dlSwitch.Bounds.W/2, dlSwitch.Bounds.Y+dlSwitch.Bounds.H/2)
	tt.Release(dlSwitch.Bounds.X+dlSwitch.Bounds.W/2, dlSwitch.Bounds.Y+dlSwitch.Bounds.H/2)
	if lastAutoDownload == nil || !*lastAutoDownload || !status.AutoDownloadUpdate {
		t.Fatalf("expected autoDownloadUpdate=true, got %+v", status)
	}

	// 3. Toggle auto-check off -> auto-download disables and displays disabled hint.
	chkSwitch, ok := tt.AccessByRole(ui.RoleSwitch, "自动检查更新")
	if !ok {
		t.Fatal("auto-check switch missing")
	}
	tt.Press(chkSwitch.Bounds.X+chkSwitch.Bounds.W/2, chkSwitch.Bounds.Y+chkSwitch.Bounds.H/2)
	tt.Release(chkSwitch.Bounds.X+chkSwitch.Bounds.W/2, chkSwitch.Bounds.Y+chkSwitch.Bounds.H/2)
	if lastAutoCheck == nil || *lastAutoCheck || status.AutoCheckUpdate {
		t.Fatalf("expected autoCheckUpdate=false, got %+v", status)
	}
	if !tt.HasText("请先开启「自动检查更新」") {
		t.Fatal("missing disabled hint when auto-check is off")
	}
	// Persisted autoDownloadUpdate remains true, while rendered switch is unchecked and disabled.
	if !status.AutoDownloadUpdate {
		t.Fatal("turning off auto-check should preserve persisted autoDownloadUpdate")
	}
	dlSwitch, ok = tt.AccessByRole(ui.RoleSwitch, "后台下载更新")
	if !ok || !dlSwitch.Disabled || dlSwitch.Checked {
		t.Fatalf("expected auto-download switch disabled and unchecked, got %+v", dlSwitch)
	}

	// 4. Switching channel to Alpha in demo mode shows warning dialog first.
	demo := NewPreview(preview.Desktop{Page: "settings", Theme: "light", Layout: "list"})
	demo.d.settings.Value = "about"
	dt := ui.NewTesterWith(demo.d.view, 1100, 900, ui.TesterOptions{})
	sel, ok := dt.AccessByRole(ui.RolePopUpButton, "更新渠道")
	if !ok {
		t.Fatal("update channel select missing")
	}
	dt.Press(sel.Bounds.X+sel.Bounds.W/2, sel.Bounds.Y+sel.Bounds.H/2)
	dt.Release(sel.Bounds.X+sel.Bounds.W/2, sel.Bounds.Y+sel.Bounds.H/2)
	if err := dt.Click("内测版"); err != nil {
		t.Fatalf("select alpha: %v", err)
	}
	if !demo.d.about.alphaWarningOpen || !dt.HasText("切换到内测版？") {
		t.Fatal("alpha warning dialog did not open")
	}
	if demo.d.currentUpdateChannel() != "stable" {
		t.Fatalf("channel changed before confirmation: %s", demo.d.currentUpdateChannel())
	}
	// Cancel keeps stable.
	if err := dt.Click("取消"); err != nil {
		t.Fatal(err)
	}
	dt.Advance(400 * time.Millisecond)
	if demo.d.currentUpdateChannel() != "stable" {
		t.Fatalf("channel changed after cancel: %s", demo.d.currentUpdateChannel())
	}
	// Re-open and confirm switches to alpha.
	dt.Advance(500 * time.Millisecond)
	sel, _ = dt.AccessByRole(ui.RolePopUpButton, "更新渠道")
	dt.Press(sel.Bounds.X+sel.Bounds.W/2, sel.Bounds.Y+sel.Bounds.H/2)
	dt.Release(sel.Bounds.X+sel.Bounds.W/2, sel.Bounds.Y+sel.Bounds.H/2)
	if err := dt.Click("内测版"); err != nil {
		t.Fatal(err)
	}
	if err := dt.Click("继续切换"); err != nil {
		t.Fatal(err)
	}
	dt.Advance(400 * time.Millisecond)
	if demo.d.currentUpdateChannel() != "alpha" {
		t.Fatalf("channel not switched to alpha: %s", demo.d.currentUpdateChannel())
	}
	if !dt.HasText("Alpha") {
		t.Fatal("channel badge did not update to Alpha")
	}
}

func TestAboutExternalOwnerAndDevBuild(t *testing.T) {
	// 1. Package-manager owner opens manual update dialog with copyable command.
	desktop, d := newTestAboutLive(t)
	d.about.sponsorsStarted = true
	d.about.sponsorsLoaded = true
	var copied, opened string
	d.about.copyText = func(s string) error { copied = s; return nil }
	desktop.SetOpenExternal(func(u string) error { opened = u; return nil })
	desktop.SetUpdater(
		func() UpdateStatus { return UpdateStatus{Supported: false, Owner: "Homebrew"} },
		nil,
		nil,
	)

	tt := ui.NewTesterWith(d.view, 1100, 900, ui.TesterOptions{})
	if !tt.HasText("此安装由 Homebrew 管理更新，应用内自动更新已关闭。") {
		t.Fatal("missing external owner notice")
	}
	if err := tt.Click("检查更新"); err != nil {
		t.Fatal(err)
	}
	if !d.about.manualUpdateOpen || !tt.HasText("请通过系统包管理器升级") || !tt.HasText("brew upgrade --cask uniclipboard") {
		t.Fatal("manual update dialog did not show Homebrew command")
	}
	if err := tt.Click("复制命令"); err != nil {
		t.Fatal(err)
	}
	if copied != "brew upgrade --cask uniclipboard" || !tt.HasText("已复制") {
		t.Fatalf("copy command failed: copied=%q", copied)
	}
	if err := tt.Click("打开发布页"); err != nil {
		t.Fatal(err)
	}
	if opened != releaseDownloadURL {
		t.Fatalf("opened=%q, want %q", opened, releaseDownloadURL)
	}

	// 2. Dev build shows disabled-in-dev notice on click.
	devDesktop, devD := newTestAboutLive(t)
	devD.about.version = "dev"
	devD.about.buildNumber = "0"
	devD.about.sponsorsStarted = true
	devD.about.sponsorsLoaded = true
	devDesktop.SetUpdater(func() UpdateStatus { return UpdateStatus{Supported: false, Owner: "direct"} }, nil, nil)
	devTT := ui.NewTesterWith(devD.view, 1100, 900, ui.TesterOptions{})
	if !devTT.HasText("Dev") || !devTT.HasText("版本 dev") {
		t.Fatal("missing Dev badge or version")
	}
	if err := devTT.Click("检查更新"); err != nil {
		t.Fatal(err)
	}
	if !devTT.HasText("当前开发构建未启用应用内更新。") {
		t.Fatal("missing dev-build update notice")
	}
}

func TestAboutSponsorsAsyncLifecycle(t *testing.T) {
	// 1. Verify JSON parsing and filtering rules.
	parsed, err := parseSponsorsJSON([]byte(`{"items":[{"id":"s1","name":"Grace Hopper","tier":"gold","url":"https://example.com/grace","avatar":"https://example.com/avatar.png"},{"id":"","name":"  "},{"id":"s2","name":"Alan Turing","note":"Great tool","tier":"regular","url":"javascript:alert(1)"}],"count":3}`))
	if err != nil || len(parsed) != 2 || parsed[0].Name != "Grace Hopper" || parsed[1].Name != "Alan Turing" {
		t.Fatalf("parseSponsorsJSON unexpected: %+v, err=%v", parsed, err)
	}

	// 2. Build a tiny valid PNG for avatar decoding test.
	var pngBuf bytes.Buffer
	img := image.NewRGBA(image.Rect(0, 0, 16, 16))
	for y := 0; y < 16; y++ {
		for x := 0; x < 16; x++ {
			img.Set(x, y, color.RGBA{R: 43, G: 127, B: 255, A: 255})
		}
	}
	if err := png.Encode(&pngBuf, img); err != nil {
		t.Fatal(err)
	}

	release := make(chan struct{})
	desktop, d := newTestAboutLive(t)
	d.about.fetchSponsors = func(ctx context.Context) ([]Sponsor, error) {
		select {
		case <-release:
			return parsed, nil
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}
	d.about.fetchAvatar = func(_ context.Context, rawURL string) ([]byte, error) {
		if rawURL == "https://example.com/avatar.png" {
			return pngBuf.Bytes(), nil
		}
		return nil, errors.New("not found")
	}
	var opened []string
	desktop.SetOpenExternal(func(u string) error { opened = append(opened, u); return nil })

	tt := ui.NewTesterWith(d.view, 1100, 900, ui.TesterOptions{})
	// Immediately visible with heading and loading skeletons while async fetch is in flight.
	if !tt.HasText("赞助者") || !tt.HasText("感谢以下朋友的赞助支持，让 UniClipboard 持续向前。") {
		t.Fatal("sponsors section should render loading state immediately")
	}
	if tt.HasText("Grace Hopper") {
		t.Fatal("sponsors should not appear before fetch settles")
	}

	close(release)
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		tt.Advance(20 * time.Millisecond)
		if d.about.sponsorsLoaded && d.about.sponsorAvatars["s1"] != nil {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if !d.about.sponsorsLoaded || len(d.about.sponsors) != 2 {
		t.Fatalf("sponsors did not load: loaded=%v count=%d", d.about.sponsorsLoaded, len(d.about.sponsors))
	}
	if d.about.sponsorAvatars["s1"] == nil {
		t.Fatal("sponsor avatar did not decode")
	}
	if !tt.HasText("Grace Hopper") || !tt.HasText("金牌") || !tt.HasText("Alan Turing") || !tt.HasText("Great tool") {
		t.Fatal("rendered sponsors missing expected text")
	}
	if err := tt.Click("Grace Hopper"); err != nil {
		t.Fatal(err)
	}
	if len(opened) != 1 || opened[0] != "https://example.com/grace" {
		t.Fatalf("unexpected opened URLs: %v", opened)
	}
	// javascript: URL must not be clickable.
	if _, ok := tt.AccessByRole(ui.RoleButton, "Alan Turing"); ok {
		t.Fatal("sponsor with javascript: URL must not be a clickable button")
	}

	// 3. Fetch failure hides the Sponsors section completely.
	_, errD := newTestAboutLive(t)
	errD.about.fetchSponsors = func(context.Context) ([]Sponsor, error) {
		return nil, errors.New("offline")
	}
	errTT := ui.NewTesterWith(errD.view, 1100, 900, ui.TesterOptions{})
	deadline = time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		errTT.Advance(20 * time.Millisecond)
		if errD.about.sponsorsLoaded {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if !errD.about.sponsorsLoaded {
		t.Fatal("failed sponsors fetch did not settle")
	}
	if errTT.HasText("赞助者") {
		t.Fatal("sponsors section must hide after fetch failure")
	}
}
