// Package webinterop checks unic against the real browser transfer page code (0022).
//
// The peer is the website's own protocol modules running on the pinned tailcat wasm
// build, driven by harness.ts under bun. Both directions run through the public
// network: the page offers and unic receives, then unic offers and the page pulls.
//
//	UNIC_WEB_INTEROP=1 UC_WEBSITE=/path/to/uc-website go test ./e2e/webinterop -v
//
// With UNIC_E2E_ARTIFACTS set, the test writes web-interop-receipt.json there.
package webinterop

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestBrowserInterop(t *testing.T) {
	site := os.Getenv("UC_WEBSITE")
	if os.Getenv("UNIC_WEB_INTEROP") == "" || site == "" {
		t.Skip("set UNIC_WEB_INTEROP=1 and UC_WEBSITE=<uc-website checkout>")
	}
	if _, err := exec.LookPath("bun"); err != nil {
		t.Skip("bun is not installed")
	}
	root := t.TempDir()
	bin := filepath.Join(root, "bin")
	run(t, "", nil, "go", "build", "-o", bin+"/", "../../app/service/cmd/unic", "../../app/service/cmd/unicd")
	home, sock := filepath.Join(root, "h"), filepath.Join(root, "s")
	if err := os.MkdirAll(home, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte("\n[telemetry]\nenabled=false\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	env := append(os.Environ(), "UNIC_HOME="+home, "UNIC_SOCK="+sock, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1", "UC_WEBSITE="+site)
	harness, err := filepath.Abs("harness.ts")
	if err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(root, "report.bin")
	payload := bytes.Repeat([]byte("0123456789abcdef"), 4096) // 64 KiB: more than one DATA frame
	if err := os.WriteFile(file, payload, 0o600); err != nil {
		t.Fatal(err)
	}
	receipt := map[string]any{"page_build": "tailcat 83921d71", "payload_bytes": len(payload)}

	t.Run("page-to-unic", func(t *testing.T) {
		began := time.Now()
		linkFile := filepath.Join(root, "page.link")
		page := exec.Command("bun", harness, "send", linkFile, "text from the page", file)
		page.Env = env
		var pageOut bytes.Buffer
		page.Stdout, page.Stderr = &pageOut, &pageOut
		if err := page.Start(); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = page.Process.Kill() })
		link := waitFile(t, linkFile, 60*time.Second, &pageOut)
		out := run(t, "", env, filepath.Join(bin, "unic"), "recv", "--json", link)
		var got struct {
			Items []struct {
				Kind, Name string
				Size       int64
				Text       string
			}
		}
		if err := json.Unmarshal([]byte(out), &got); err != nil {
			t.Fatalf("recv output: %v\n%s", err, out)
		}
		if len(got.Items) != 2 || got.Items[0].Kind != "text" || got.Items[0].Text != "text from the page" ||
			got.Items[1].Kind != "file" || got.Items[1].Name != "report.bin" || got.Items[1].Size != int64(len(payload)) {
			t.Fatalf("unexpected items: %s", out)
		}
		waitExit(t, page, 30*time.Second)
		if !strings.Contains(pageOut.String(), "DELIVERED") {
			t.Fatalf("the page did not report delivery:\n%s", pageOut.String())
		}
		hist := run(t, "", env, filepath.Join(bin, "unic"), "history", "list")
		if !strings.Contains(hist, "external:unic.transfer") {
			t.Fatalf("history does not show the external origin:\n%s", hist)
		}
		receipt["page_to_unic_ms"] = time.Since(began).Milliseconds()
	})

	t.Run("unic-to-page", func(t *testing.T) {
		began := time.Now()
		var stdout, stderr bytes.Buffer
		send := exec.Command(filepath.Join(bin, "unic"), "send", "text from unic")
		send.Env = env
		send.Stdout, send.Stderr = &stdout, &stderr
		if err := send.Start(); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = send.Process.Kill() })
		link := ""
		deadline := time.Now().Add(60 * time.Second)
		for time.Now().Before(deadline) && link == "" {
			for _, ln := range strings.Split(stdout.String(), "\n") {
				if v, ok := strings.CutPrefix(ln, "Link: "); ok && strings.HasSuffix(stdout.String(), "\n") {
					link = v
				}
			}
			time.Sleep(200 * time.Millisecond)
		}
		if link == "" {
			t.Fatalf("unic printed no link:\n%s\n%s", stdout.String(), stderr.String())
		}
		out := run(t, "", env, "bun", harness, "recv", link)
		var items []struct {
			Kind, Name, Mime, Text string
			Size                   int64
		}
		last := out[strings.LastIndex(strings.TrimSpace(out), "\n")+1:]
		if err := json.Unmarshal([]byte(last), &items); err != nil {
			t.Fatalf("page output: %v\n%s", err, out)
		}
		if len(items) != 1 || items[0].Kind != "text" || items[0].Text != "text from unic" {
			t.Fatalf("unexpected items: %s", last)
		}
		waitExit(t, send, 30*time.Second)
		if !strings.Contains(stderr.String(), "delivered") {
			t.Fatalf("unic did not report delivery:\n%s", stderr.String())
		}
		receipt["unic_to_page_ms"] = time.Since(began).Milliseconds()
	})

	if dir := os.Getenv("UNIC_E2E_ARTIFACTS"); dir != "" {
		_ = os.MkdirAll(dir, 0o700)
		b, _ := json.MarshalIndent(receipt, "", "  ")
		if err := os.WriteFile(filepath.Join(dir, "web-interop-receipt.json"), append(b, '\n'), 0o600); err != nil {
			t.Fatal(err)
		}
	}
}

func run(t *testing.T, dir string, env []string, name string, args ...string) string {
	t.Helper()
	cmd := exec.Command(name, args...)
	cmd.Dir, cmd.Env = dir, env
	var o, e bytes.Buffer
	cmd.Stdout, cmd.Stderr = &o, &e
	if err := cmd.Run(); err != nil {
		t.Fatalf("%s %s: %v\n%s\n%s", name, strings.Join(args[:min(len(args), 2)], " "), err, o.String(), e.String())
	}
	return o.String()
}

func waitFile(t *testing.T, path string, d time.Duration, log fmt.Stringer) string {
	t.Helper()
	for end := time.Now().Add(d); time.Now().Before(end); time.Sleep(200 * time.Millisecond) {
		if b, err := os.ReadFile(path); err == nil && len(b) > 0 {
			return strings.TrimSpace(string(b))
		}
	}
	t.Fatalf("no link within %s\n%s", d, log.String())
	return ""
}

func waitExit(t *testing.T, c *exec.Cmd, d time.Duration) {
	t.Helper()
	done := make(chan error, 1)
	go func() { done <- c.Wait() }()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("%s exited with %v", filepath.Base(c.Path), err)
		}
	case <-time.After(d):
		t.Fatalf("%s did not exit within %s", filepath.Base(c.Path), d)
	}
}
