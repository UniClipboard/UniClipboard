package desktopui

import (
	"bytes"
	"context"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	api "github.com/uniclipboard/unic/internal/localapi"
)

// TestLiveMarketplacePrebuiltInstallAndDesktopViews exercises the real remote
// UniClipboard/unic-plugins registry.json, real prebuilt v0.1.0 GitHub Release
// download/SHA-256 install (Build=false, no Go toolchain needed), real unicd
// restart + RPC handshake of unic.wenshushu, and captures real BoardUI views.
// Opt-in via UNIC_LIVE_MARKETPLACE_E2E=1 so offline CI remains deterministic.
func TestLiveMarketplacePrebuiltInstallAndDesktopViews(t *testing.T) {
	if os.Getenv("UNIC_LIVE_MARKETPLACE_E2E") != "1" {
		t.Skip("set UNIC_LIVE_MARKETPLACE_E2E=1 to run live GitHub marketplace & prebuilt release E2E")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()

	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("/private/tmp", "umkt-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = os.RemoveAll(private) }()

	outDir := os.Getenv("UNIC_MARKETPLACE_SHOTS")
	saveShot := func(tt *ui.Tester, name string) {
		if outDir == "" {
			return
		}
		if err := os.MkdirAll(outDir, 0700); err != nil {
			t.Fatal(err)
		}
		f, err := os.Create(filepath.Join(outDir, name))
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = f.Close() }()
		if err := png.Encode(f, tt.Image()); err != nil {
			t.Fatal(err)
		}
	}

	exe := filepath.Join(private, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build unicd: %v\n%s", err, out)
	}

	home := filepath.Join(private, "home")
	if err := os.MkdirAll(home, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled = false\n[sync]\nenabled = false\n"), 0600); err != nil {
		t.Fatal(err)
	}

	baseEnv := []string{}
	for _, v := range os.Environ() {
		if !strings.HasPrefix(v, "UNIC_") {
			baseEnv = append(baseEnv, v)
		}
	}
	baseEnv = append(baseEnv, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")

	startService := func() (*exec.Cmd, *bytes.Buffer) {
		var logs bytes.Buffer
		cmd := exec.CommandContext(ctx, exe)
		cmd.Env = slices.Clone(baseEnv)
		cmd.Stdout = &logs
		cmd.Stderr = &logs
		if err := cmd.Start(); err != nil {
			t.Fatal(err)
		}
		client := api.NewClient(filepath.Join(home, "unic.sock"))
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			if _, err := client.Status(ctx); err == nil {
				return cmd, &logs
			}
			time.Sleep(50 * time.Millisecond)
		}
		_ = cmd.Process.Kill()
		_ = cmd.Wait()
		t.Fatalf("unicd failed to start:\n%s", logs.String())
		return nil, nil
	}

	stopService := func(cmd *exec.Cmd, logs *bytes.Buffer) {
		if cmd == nil || cmd.Process == nil {
			return
		}
		_ = cmd.Process.Signal(os.Interrupt)
		done := make(chan struct{})
		go func() {
			_ = cmd.Wait()
			close(done)
		}()
		select {
		case <-done:
		case <-time.After(8 * time.Second):
			_ = cmd.Process.Kill()
			<-done
		}
		if t.Failed() && logs != nil {
			t.Logf("unicd logs:\n%s", logs.String())
		}
	}

	cmd, logs := startService()
	defer func() { stopService(cmd, logs) }()

	client := api.NewClient(filepath.Join(home, "unic.sock"))

	// 1. Verify live remote registry.json fetch.
	cat, err := client.PluginMarketplace(ctx, true)
	if err != nil {
		t.Fatalf("PluginMarketplace: %v", err)
	}
	if cat.Source != "remote" || cat.Warning != "" || len(cat.Plugins) == 0 || cat.Plugins[0].ID != "unic.wenshushu" {
		t.Fatalf("expected live remote catalog with unic.wenshushu, got: %+v", cat)
	}
	ws := cat.Plugins[0]

	// 2. Render fresh Desktop Plugins + live Marketplace views.
	desktop := NewDesktop(ctx, client)
	d := desktop.d
	d.page = "settings"
	d.settings.Value = "plugins"
	tt := ui.NewTesterWith(desktop.View, 1120, 820, ui.TesterOptions{})

	wait := func(label string, cond func() bool) {
		t.Helper()
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			d.live.poll(d)
			tt.Advance(25 * time.Millisecond)
			if cond() {
				return
			}
			time.Sleep(25 * time.Millisecond)
		}
		t.Fatalf("timeout waiting for %s", label)
	}

	wait("fresh plugins page", func() bool {
		return d.live.loaded && d.live.marketplaceLoaded && tt.HasText("插件市场") && tt.HasText("文叔叔临时分享")
	})
	saveShot(tt, "01-plugins-main-fresh.png")

	if err := tt.Click("浏览插件市场"); err != nil {
		t.Fatalf("click 浏览插件市场: %v", err)
	}
	wait("live marketplace view", func() bool {
		return d.pluginSettings.view == "marketplace" && tt.HasText("如何使用") && tt.HasText("隐私与安全")
	})
	saveShot(tt, "02-marketplace-live.png")

	// 3. Install Wenshushu using prebuilt Release asset ONLY (Build=false!).
	info, err := client.InstallPlugin(ctx, api.PluginInstallRequest{
		Repository: ws.Repository,
		Ref:        ws.Ref,
		Path:       ws.Path,
		Build:      false,
		Enable:     true,
	})
	if err != nil {
		t.Fatalf("prebuilt InstallPlugin failed: %v", err)
	}
	if info.ID != "unic.wenshushu" || info.Source != "prebuilt" || !info.Configured {
		t.Fatalf("expected prebuilt enabled unic.wenshushu, got: %+v", info)
	}
	desktop.Close()

	// 4. Restart unicd to start the real installed prebuilt unic.wenshushu process.
	stopService(cmd, logs)
	cmd, logs = startService()

	plugins, err := client.Plugins(ctx)
	if err != nil {
		t.Fatalf("Plugins after restart: %v", err)
	}
	if len(plugins) != 1 || plugins[0].ID != "unic.wenshushu" || plugins[0].RunningVersion != ws.Version || plugins[0].Source != "prebuilt" || plugins[0].Restart {
		t.Fatalf("expected running prebuilt unic.wenshushu after restart, got: %+v", plugins)
	}
	actions, err := client.Actions(ctx)
	if err != nil {
		t.Fatalf("Actions after restart: %v", err)
	}
	foundShare := false
	for _, a := range actions {
		if a.Key == "unic.wenshushu/share" && a.Title == "通过文叔叔临时分享" {
			foundShare = true
		}
	}
	if !foundShare {
		t.Fatalf("expected unic.wenshushu/share in live Engine actions, got: %+v", actions)
	}

	// 5. Reconnect Desktop and capture installed main page + running Wenshushu detail.
	desktop2 := NewDesktop(ctx, client)
	defer desktop2.Close()
	d2 := desktop2.d
	d2.page = "settings"
	d2.settings.Value = "plugins"
	tt2 := ui.NewTesterWith(desktop2.View, 1120, 820, ui.TesterOptions{})

	wait2 := func(label string, cond func() bool) {
		t.Helper()
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			d2.live.poll(d2)
			tt2.Advance(25 * time.Millisecond)
			if cond() {
				return
			}
			time.Sleep(25 * time.Millisecond)
		}
		t.Fatalf("timeout waiting for %s", label)
	}

	wait2("installed plugins list", func() bool {
		return d2.live.loaded && d2.live.marketplaceLoaded && len(d2.live.externalPlugins) == 1 && tt2.HasText("文叔叔临时分享")
	})
	saveShot(tt2, "03-plugins-main-installed.png")

	if err := tt2.Click("unic.wenshushu"); err != nil {
		t.Fatalf("click unic.wenshushu detail button: %v", err)
	}
	wait2("running Wenshushu detail", func() bool {
		return d2.pluginSettings.id == "unic.wenshushu" && tt2.HasText("运行中") && tt2.HasText("通过文叔叔临时分享")
	})
	if tt2.HasText("插件设置") {
		t.Fatalf("zero-config plugin must not show 插件设置: %v", tt2.Texts())
	}
	saveShot(tt2, "04-wenshushu-running-detail.png")
}
