//go:build unic_e2e

package headless

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/nativeboard"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func TestMacOSDaemonClipboard(t *testing.T) {
	artifacts := os.Getenv("UNIC_MACOS_CLIPBOARD_ARTIFACTS")
	if artifacts == "" {
		t.Skip("set UNIC_MACOS_CLIPBOARD_ARTIFACTS to a receipt directory")
	}
	need(t, os.MkdirAll(artifacts, 0700))
	root, err := filepath.Abs("../..")
	need(t, err)
	sources, err := sourceHashes(root)
	need(t, err)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	work, err := os.MkdirTemp("/tmp", "unic-board-")
	need(t, err)
	t.Cleanup(func() { _ = os.RemoveAll(work) })
	bundle := filepath.Join(work, "Pairing Fixture.app")
	contents := filepath.Join(bundle, "Contents")
	programs := filepath.Join(contents, "MacOS")
	need(t, os.MkdirAll(programs, 0700))
	plist := []byte(`<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>org.uniclipboard.e2e.notifications</string>
<key>CFBundleExecutable</key><string>unic-desktop</string>
<key>CFBundleName</key><string>UniClipboard E2E</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>`)
	need(t, os.WriteFile(filepath.Join(contents, "Info.plist"), plist, 0600))
	binary := filepath.Join(programs, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-tags", "unic_e2e", "-o", binary, "./app/service/cmd/unicd")
	build.Dir = root
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build unicd: %v\n%s", err, output)
	}
	payload, err := os.ReadFile(binary)
	need(t, err)
	need(t, os.WriteFile(filepath.Join(programs, "unic-desktop"), payload, 0700))
	sign := exec.CommandContext(ctx, "codesign", "--force", "--sign", "-", bundle)
	if output, err := sign.CombinedOutput(); err != nil {
		t.Fatalf("sign fixture: %v\n%s", err, output)
	}
	payload, err = os.ReadFile(binary)
	need(t, err)
	var checks []string
	defer func() {
		receipt := map[string]any{"source": sources, "binary_sha256": fmt.Sprintf("%x", sha256.Sum256(payload)),
			"checks": checks, "passed": !t.Failed()}
		data, err := json.MarshalIndent(receipt, "", "  ")
		need(t, err)
		need(t, os.WriteFile(filepath.Join(artifacts, "receipt.json"), data, 0600))
	}()

	for _, tc := range []struct {
		name string
		arg  string
	}{
		{"standalone", ""},
		{"temporary", "--cli-temporary"},
		{"desktop", "--desktop-owned"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			dir := filepath.Join(work, tc.name)
			need(t, os.Mkdir(dir, 0700))
			name := "org.uniclipboard.e2e." + rand.Text()
			board, err := nativeboard.NamedForE2E(name)
			need(t, err)
			t.Cleanup(func() { _ = board.ReleaseForE2E() })
			write := func(text string) {
				t.Helper()
				need(t, board.Write(clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(text)}}}))
			}
			write("baseline")
			need(t, os.WriteFile(filepath.Join(dir, "config.toml"), []byte("[sync]\nenabled = false\n[telemetry]\nenabled = false\n"), 0600))
			socket := filepath.Join(dir, "u.sock")
			var args []string
			if tc.arg != "" {
				args = append(args, tc.arg)
			}
			daemon := exec.CommandContext(ctx, binary, args...)
			daemon.Env = append(os.Environ(), "UNIC_HOME="+dir, "UNIC_SOCK="+socket, "UNIC_E2E_NAMED_PASTEBOARD="+name)
			stdin, err := daemon.StdinPipe()
			need(t, err)
			log, err := os.OpenFile(filepath.Join(artifacts, tc.name+".log"), os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0600)
			need(t, err)
			daemon.Stdout, daemon.Stderr = log, log
			need(t, daemon.Start())
			defer func() {
				_ = daemon.Process.Signal(syscall.SIGTERM)
				_ = daemon.Wait()
				_ = log.Close()
			}()
			client := api.NewClient(socket)
			var service servicelifecycle.Instance
			until(t, "service readiness", func() bool {
				service, err = client.Service(ctx)
				return err == nil && service.PID == daemon.Process.Pid
			})
			if _, err := client.Clipboard(ctx); err == nil {
				t.Fatal("clipboard enabled before Desktop opt-in")
			}
			checks = append(checks, tc.name+": startup has no adapter")
			notifications, err := client.EnableDesktopNotifications(ctx)
			need(t, err)
			if notifications != (tc.name != "temporary") {
				t.Fatalf("notification routing is wrong for %s: %t", tc.name, notifications)
			}
			checks = append(checks, tc.name+": notification ownership honors launch mode")
			if tc.name == "temporary" {
				if err := client.EnableDesktopClipboard(ctx); err == nil {
					t.Fatal("CLI temporary service allowed clipboard integration")
				}
				checks = append(checks, "temporary: cannot enable the adapter")
				return
			}
			need(t, client.EnableDesktopClipboard(ctx))
			need(t, client.EnableDesktopClipboard(ctx))
			var adapterSession string
			err = client.AdapterCall(ctx, "", "register", api.AdapterRegistration{}, &adapterSession)
			var adapterError *engine.Error
			if !errors.As(err, &adapterError) || adapterError.Code != engine.CodeInUse {
				t.Fatalf("second adapter registration: %v", err)
			}
			page, err := client.ListHistory(ctx, "", "")
			need(t, err)
			if len(page.Items) != 0 {
				t.Fatal("startup baseline entered history")
			}
			checks = append(checks, tc.name+": idempotent enable and exclusive baseline")
			var pngBytes bytes.Buffer
			need(t, png.Encode(&pngBytes, image.NewNRGBA(image.Rect(0, 0, 4, 4))))
			content := clip.Snapshot{Parts: []clip.Part{
				{Format: clip.Text, Data: []byte("native text")},
				{Format: clip.HTML, Data: []byte("<b>native text</b>")},
				{Format: clip.PNG, Data: pngBytes.Bytes()},
			}}
			need(t, board.Write(content))
			until(t, "native clipboard capture", func() bool {
				page, err = client.ListHistory(ctx, "", "")
				return err == nil && len(page.Items) == 1 && len(page.Items[0].Parts) == 3
			})
			first := page.Items[0].ID
			path := filepath.Join(dir, "file.txt")
			need(t, os.WriteFile(path, []byte("hello"), 0600))
			need(t, board.Write(clip.Snapshot{Files: []clip.File{{Name: "file.txt", Size: 5, Path: path}}}))
			until(t, "native file capture", func() bool {
				page, err = client.ListHistory(ctx, "", "")
				return err == nil && len(page.Items) == 2 && page.Items[0].Kind == "files"
			})
			refPath := filepath.Join(dir, "finder-file.txt")
			need(t, os.WriteFile(refPath, []byte("finder"), 0600))
			need(t, board.WriteFileReferencesForE2E(refPath))
			until(t, "Finder file reference capture", func() bool {
				page, err = client.ListHistory(ctx, "", "")
				return err == nil && len(page.Items) == 3 && page.Items[0].Kind == "files" &&
					len(page.Items[0].Parts) == 1 && page.Items[0].Parts[0].Kind == "file" && page.Items[0].Parts[0].Size == 6
			})
			out, err := client.ApplyDesktopClipboard(ctx, first, false)
			need(t, err)
			if out.Failure != "" || out.Mark == "" {
				t.Fatalf("tracked application failed: %+v", out)
			}
			match, err := client.MatchDesktopClipboard(ctx, out.Mark)
			need(t, err)
			got, err := board.Read()
			need(t, err)
			if !match || got.Hash() != content.Hash() {
				t.Fatal("history write-back did not preserve the clipboard bytes")
			}
			checks = append(checks, tc.name+": native text, HTML, PNG, file capture and tracked write-back")
			if tc.name == "desktop" {
				_, err := stdin.Write([]byte(servicelifecycle.OwnerReleaseMarker))
				need(t, err)
				need(t, stdin.Close())
				until(t, "ownership release", func() bool {
					info, err := client.Service(ctx)
					return err == nil && info.Mode == servicelifecycle.ModeStandalone && info.ID == service.ID && info.PID == service.PID
				})
			}
			write("new copy")
			match, err = client.MatchDesktopClipboard(ctx, out.Mark)
			need(t, err)
			if match {
				t.Fatal("stale paste ownership match")
			}
			until(t, "native capture after startup or handoff, without write-back echo", func() bool {
				page, err = client.ListHistory(ctx, "", "")
				return err == nil && len(page.Items) == 4
			})
			checks = append(checks, tc.name+": stale receipt rejected and capture continues without echo")
		})
	}
}
