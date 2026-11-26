//go:build windows || linux

package headless

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/png"
	"math/rand/v2"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"golang.design/x/clipboard"

	api "github.com/uniclipboard/unic/internal/localapi"
)

var matchClipboardDuringHandoff = (*api.Client).MatchDesktopClipboard

func TestDaemonNativeDesktopClipboard(t *testing.T) {
	if os.Getenv("UNIC_NATIVE_CLIPBOARD_E2E") != "1" {
		t.Skip("requires an isolated interactive Windows, X11, or Wayland session")
	}
	binary := os.Getenv("UNIC_NATIVE_CLIPBOARD_DAEMON")
	outDir := os.Getenv("UNIC_NATIVE_CLIPBOARD_ARTIFACTS")
	if binary == "" || outDir == "" {
		t.Fatal("set UNIC_NATIVE_CLIPBOARD_DAEMON and UNIC_NATIVE_CLIPBOARD_ARTIFACTS")
	}
	need(t, os.MkdirAll(outDir, 0700))
	checks := []string{}
	t.Cleanup(func() {
		outcome := "pass"
		if t.Failed() {
			outcome = "fail"
		}
		data, err := json.MarshalIndent(map[string]any{
			"platform": runtime.GOOS, "arch": runtime.GOARCH,
			"wayland": os.Getenv("WAYLAND_DISPLAY") != "", "status": outcome, "checks": checks,
		}, "", "  ")
		need(t, err)
		need(t, os.WriteFile(filepath.Join(outDir, "manifest.json"), data, 0600))
		need(t, os.WriteFile(filepath.Join(outDir, "SHA256SUMS"), fmt.Appendf(nil, "%x  manifest.json\n", sha256.Sum256(data)), 0600))
	})
	ctx, cancel := context.WithTimeout(context.Background(), 85*time.Second)
	defer cancel()
	dir := t.TempDir()
	socket := filepath.Join(dir, "u.sock")
	need(t, os.WriteFile(filepath.Join(dir, "config.toml"), []byte("[sync]\nenabled = false\n[telemetry]\nenabled = false\n"), 0600))
	daemon := start(t, binary, nil, append(os.Environ(), "UNIC_HOME="+dir, "UNIC_SOCK="+socket), filepath.Join(outDir, "unicd.log"))
	client := api.NewClient(socket)
	until(t, "standalone unicd", func() bool {
		s, err := client.Service(ctx)
		return err == nil && s.PID == daemon.cmd.Process.Pid
	})
	if _, err := client.Clipboard(ctx); err == nil {
		t.Fatal("standalone start enabled clipboard access")
	}
	checks = append(checks, "headless before desktop opt-in")
	if os.Getenv("UNIC_NATIVE_CLIPBOARD_EXPECT_UNSUPPORTED") == "1" {
		err := client.EnableDesktopClipboard(ctx)
		if api.ReasonOf(err) != "clipboard_unsupported" {
			t.Fatalf("unsupported desktop: %v", err)
		}
		if _, err := client.Clipboard(ctx); err == nil {
			t.Fatal("unsupported session acquired the adapter")
		}
		checks = append(checks, "unsupported session leaves daemon headless")
		return
	}
	need(t, clipboard.Init())
	write := func(text string) {
		t.Helper()
		_, err := clipboard.Write(ctx, clipboard.FmtText, []byte(text))
		need(t, err)
	}
	write("unicd clipboard migration E2E startup baseline")
	need(t, client.EnableDesktopClipboard(ctx))
	need(t, client.EnableDesktopClipboard(ctx))
	checks = append(checks, "one adapter after idempotent desktop opt-in")
	before, err := client.ListHistory(ctx, "", "")
	need(t, err)
	if len(before.Items) != 0 {
		t.Fatal("existing clipboard entered history on enable")
	}
	var pngBytes bytes.Buffer
	need(t, png.Encode(&pngBytes, image.NewNRGBA(image.Rect(0, 0, 4, 4))))
	text := "unicd clipboard migration E2E payload"
	_, err = clipboard.WriteAll(ctx,
		clipboard.Item{Format: clipboard.FmtText, Bytes: []byte(text)},
		clipboard.Item{Format: clipboard.Register("text/html"), Bytes: []byte("<b>" + text + "</b>")},
		clipboard.Item{Format: clipboard.FmtImage, Bytes: pngBytes.Bytes()},
	)
	need(t, err)
	var id string
	until(t, "desktop clipboard observed by unicd without a tray", func() bool {
		page, err := client.ListHistory(ctx, "", "")
		if err != nil || len(page.Items) != 1 || len(page.Items[0].Parts) != 3 {
			return false
		}
		id = page.Items[0].ID
		return true
	})
	checks = append(checks, "text HTML and PNG captured without a tray")
	path := filepath.Join(dir, "clipboard-e2e.txt")
	need(t, os.WriteFile(path, []byte("fixture"), 0600))
	_, err = clipboard.WriteFiles(ctx, []string{path})
	need(t, err)
	until(t, "native files observed by unicd", func() bool {
		page, err := client.ListHistory(ctx, "", "")
		return err == nil && len(page.Items) == 2 && page.Items[0].Kind == "files"
	})
	out, err := client.ApplyDesktopClipboard(ctx, id, false)
	need(t, err)
	if out.Mark == "" || out.Failure != "" {
		t.Fatalf("apply history through daemon: %+v", out)
	}
	match, err := matchClipboardDuringHandoff(client, ctx, out.Mark)
	need(t, err)
	got, err := clipboard.Read(ctx, clipboard.FmtText)
	need(t, err)
	if !match || string(got) != text {
		t.Fatal("native clipboard did not match the applied item")
	}
	checks = append(checks, "file capture and verified apply through unicd")
	write("unicd clipboard migration E2E external replacement")
	match, err = client.MatchDesktopClipboard(ctx, out.Mark)
	need(t, err)
	if match {
		t.Fatal("clipboard replacement left the paste mark valid")
	}
	until(t, "external replacement observed without write-back echo", func() bool {
		page, err := client.ListHistory(ctx, "", "")
		return err == nil && len(page.Items) == 3
	})
	checks = append(checks, "replacement invalidates paste mark without observation echo")
	if runtime.GOOS == "linux" && os.Getenv("UNIC_NATIVE_CLIPBOARD_X11_INCR") == "1" {
		var large bytes.Buffer
		pixels := image.NewNRGBA(image.Rect(0, 0, 512, 512))
		random := rand.NewChaCha8([32]byte{5})
		_, _ = random.Read(pixels.Pix)
		need(t, png.Encode(&large, pixels))
		cmd := exec.CommandContext(ctx, "xclip", "-selection", "clipboard", "-target", "image/png", "-quiet")
		cmd.Stdin = bytes.NewReader(large.Bytes())
		cmd.Stderr = os.Stderr
		need(t, cmd.Start())
		defer func() { _ = cmd.Process.Kill(); _ = cmd.Wait() }()
		var largeID string
		until(t, "X11 incrementally transferred image captured in full", func() bool {
			page, err := client.ListHistory(ctx, "", "")
			if err != nil || len(page.Items) != 4 || len(page.Items[0].Parts) != 1 || page.Items[0].Parts[0].Size != int64(large.Len()) {
				return false
			}
			largeID = page.Items[0].ID
			return true
		})
		out, err := client.ApplyDesktopClipboard(ctx, largeID, false)
		need(t, err)
		if out.Failure != "" {
			t.Fatalf("apply large X11 image: %s", out.Failure)
		}
		got, err := exec.CommandContext(ctx, "xclip", "-selection", "clipboard", "-target", "image/png", "-out").Output()
		need(t, err)
		if !bytes.Equal(got, large.Bytes()) {
			t.Fatal("large X11 image was changed or truncated")
		}
		checks = append(checks, "large X11 image sent and received using INCR")
	}

	daemon.stop()
	daemon = start(t, binary, nil, append(os.Environ(), "UNIC_HOME="+dir, "UNIC_SOCK="+socket), filepath.Join(outDir, "restart.log"))
	until(t, "restarted headless unicd", func() bool {
		s, err := client.Service(ctx)
		return err == nil && s.PID == daemon.cmd.Process.Pid
	})
	if _, err := client.Clipboard(ctx); err == nil || errors.Is(err, api.ErrNoDaemon) {
		t.Fatalf("expected clipboard disabled after restart, got %v", err)
	}
	checks = append(checks, "restart requires a new opt-in")
}
