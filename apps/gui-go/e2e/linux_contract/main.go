// Command linux_contract checks, on any host, the Linux pieces whose behaviour is protocol or data handling:
//   - the Hyprland IPC client (paste-to-previous-app) against a scripted compositor socket, including the failure
//     modes of the Tauri implementation (reused window address, hostile address, rejected dispatch, silent
//     compositor, oversized reply, focus never confirmed);
//   - the AppImage update payload (tar.gz and bare ELF), the in-place replacement and its refusals.
//
// The scripted socket stands in for Hyprland: it proves what the client sends and how it reacts, NOT that a real
// Hyprland accepts the `hl.dsp.*` dispatch syntax or that a key reaches an application. It writes
// `linux-contract-assertions.json` into the directory given as the only argument and exits non-zero on failure.
package main

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hyprland"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

type assertion struct {
	Name   string `json:"name"`
	Passed bool   `json:"passed"`
	Detail string `json:"detail,omitempty"`
}

var results []assertion

func check(name string, ok bool, detail string) {
	results = append(results, assertion{name, ok, detail})
}

// compositor answers each connection with the next scripted reply and records the commands it received. A reply
// (the last one repeats) of "<hang>" keeps the connection open without answering; "<big>" sends more than the client accepts.
type compositor struct {
	path     string
	listener net.Listener
	mu       sync.Mutex
	commands []string
	replies  []string
}

func newCompositor(dir, name string, replies ...string) *compositor {
	path := filepath.Join(dir, name)
	l, err := net.Listen("unix", path)
	if err != nil {
		panic(err)
	}
	c := &compositor{path: path, listener: l, replies: replies}
	go func() {
		for {
			conn, err := l.Accept()
			if err != nil {
				return
			}
			go c.serve(conn)
		}
	}()
	return c
}

func (c *compositor) serve(conn net.Conn) {
	defer conn.Close()
	buf := make([]byte, 4096)
	n, _ := conn.Read(buf)
	c.mu.Lock()
	c.commands = append(c.commands, string(buf[:n]))
	var reply string
	if len(c.replies) > 1 {
		reply, c.replies = c.replies[0], c.replies[1:]
	} else if len(c.replies) == 1 {
		reply = c.replies[0] // the last reply repeats
	}
	c.mu.Unlock()
	switch reply {
	case "<hang>":
		time.Sleep(2 * time.Second)
	case "<big>":
		_, _ = conn.Write(bytes.Repeat([]byte("x"), 2<<20))
	default:
		_, _ = io.WriteString(conn, reply)
	}
}

func (c *compositor) sent() []string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]string{}, c.commands...)
}

func (c *compositor) close() { c.listener.Close() }

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: linux_contract <outdir>")
		os.Exit(2)
	}
	dir, err := os.MkdirTemp("", "uc-linux-contract-")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(dir)
	hyprlandChecks(dir)
	appImageChecks(dir)

	failed := 0
	for _, r := range results {
		if !r.Passed {
			failed++
			fmt.Fprintf(os.Stderr, "FAIL %s: %s\n", r.Name, r.Detail)
		}
	}
	raw, _ := json.MarshalIndent(map[string]any{"failed": failed, "total": len(results), "assertions": results}, "", "  ")
	if err := os.WriteFile(filepath.Join(os.Args[1], "linux-contract-assertions.json"), raw, 0o644); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("%d/%d assertions passed\n", len(results)-failed, len(results))
	if failed > 0 {
		os.Exit(1)
	}
}

const kitty = `{"address":"0x123","pid":42,"class":"kitty"}`

func hyprlandChecks(dir string) {
	target := hyprland.WindowTarget{Address: "0x123", PID: 42, Class: "kitty"}

	c := newCompositor(dir, "ok.sock", `[`+kitty+`]`, "ok", kitty, "ok")
	client := hyprland.NewAt(c.path)
	err := client.Focus(target)
	if err == nil {
		err = client.SendPaste(target)
	}
	sent := c.sent()
	check("focus then paste succeeds on a matching window", err == nil && len(sent) == 4, fmt.Sprint(err, sent))
	check("command order is clients, focus, activewindow, send_shortcut",
		len(sent) == 4 && sent[0] == "j/clients" && strings.Contains(sent[1], "hl.dsp.focus") && sent[2] == "j/activewindow" &&
			strings.Contains(sent[3], "send_shortcut"), fmt.Sprint(sent))
	check("terminal windows get CTRL SHIFT and the target address", len(sent) == 4 &&
		strings.Contains(sent[3], `mods = "CTRL SHIFT"`) && strings.Contains(sent[3], `address:0x123`), fmt.Sprint(sent))
	c.close()

	check("regular applications use CTRL", hyprland.PasteModifiers("chromium") == "CTRL" && hyprland.PasteModifiers("Alacritty") == "CTRL SHIFT" &&
		hyprland.PasteModifiers("com.mitchellh.ghostty") == "CTRL SHIFT", "")

	c = newCompositor(dir, "reuse.sock", `[{"address":"0x123","pid":99,"class":"kitty"}]`)
	err = hyprland.NewAt(c.path).Focus(target)
	check("a reused window address (different pid) is not focused", err != nil && len(c.sent()) == 1, fmt.Sprint(err, c.sent()))
	c.close()

	c = newCompositor(dir, "gone.sock", `[]`)
	err = hyprland.NewAt(c.path).Focus(target)
	check("a window that disappeared is reported", err != nil && strings.Contains(err.Error(), "no longer available"), fmt.Sprint(err))
	c.close()

	hostile := 0
	for _, address := range []string{"", "0x0", "0x", "0x123\"}", "0x123;exec", "0x12345678901234567", "123"} {
		c = newCompositor(dir, "hostile.sock", "ok")
		if hyprland.NewAt(c.path).SendPaste(hyprland.WindowTarget{Address: address, PID: 42}) != nil && len(c.sent()) == 0 {
			hostile++
		}
		c.close()
		os.Remove(filepath.Join(dir, "hostile.sock"))
	}
	check("addresses that could escape the dispatcher argument never reach the socket", hostile == 7, fmt.Sprint(hostile))
	check("a valid identity passes validation", hyprland.WindowTarget{Address: "0xabcd1234", PID: 42}.Validate() == nil, "")

	c = newCompositor(dir, "reject.sock", "error: bad", "ok")
	err = hyprland.NewAt(c.path).SendPaste(target)
	check("a rejected dispatch fails without echoing compositor output", err != nil && !strings.Contains(err.Error(), "bad"), fmt.Sprint(err))
	c.close()

	c = newCompositor(dir, "silent.sock", "<hang>")
	start := time.Now()
	err = hyprland.NewAt(c.path).SendPaste(target)
	check("a compositor that never replies is bounded by the deadline", err != nil && time.Since(start) < time.Second, fmt.Sprint(err, time.Since(start)))
	c.close()

	c = newCompositor(dir, "big.sock", "<big>")
	_, err = hyprland.NewAt(c.path).ActiveWindow()
	check("an oversized reply is refused", err != nil, fmt.Sprint(err))
	c.close()

	c = newCompositor(dir, "none.sock", "{}")
	active, err := hyprland.NewAt(c.path).ActiveWindow()
	check("no active window is reported as nil, not an error", err == nil && active == nil, fmt.Sprint(err))
	c.close()

	c = newCompositor(dir, "unfocused.sock", `[`+kitty+`]`, "ok", `{"address":"0x999","pid":7,"class":"x"}`)
	start = time.Now()
	err = hyprland.NewAt(c.path).Focus(target)
	check("focus that is never confirmed fails after the 500 ms deadline", err != nil && strings.Contains(err.Error(), "could not be confirmed") &&
		time.Since(start) >= 450*time.Millisecond, fmt.Sprint(err, time.Since(start)))
	c.close()

	check("no socket at all reports a connection error", func() bool {
		_, err := hyprland.NewAt(filepath.Join(dir, "absent.sock")).ActiveWindow()
		return err != nil
	}(), "")
	os.Unsetenv("HYPRLAND_INSTANCE_SIGNATURE")
	check("outside Hyprland there is no client", hyprland.Current() == nil, "")
	os.Setenv("XDG_RUNTIME_DIR", dir)
	os.Setenv("HYPRLAND_INSTANCE_SIGNATURE", "../escape")
	check("a signature with path characters is rejected", hyprland.Current() == nil, "")
	os.Setenv("HYPRLAND_INSTANCE_SIGNATURE", "abc_123-x")
	check("a plain signature selects the per-instance socket", hyprland.Current() != nil, "")
}

func tarGz(files map[string][]byte) []byte {
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	for name, data := range files {
		_ = tw.WriteHeader(&tar.Header{Name: name, Mode: 0o755, Size: int64(len(data)), Typeflag: tar.TypeReg})
		_, _ = tw.Write(data)
	}
	_ = tw.Close()
	_ = gz.Close()
	return buf.Bytes()
}

func appImageChecks(dir string) {
	elf := append([]byte{0x7f, 'E', 'L', 'F'}, bytes.Repeat([]byte{1}, 64)...)
	got, err := update.ExtractAppImage(elf)
	check("a bare ELF AppImage is accepted as is", err == nil && bytes.Equal(got, elf), fmt.Sprint(err))
	got, err = update.ExtractAppImage(tarGz(map[string][]byte{"UniClipboard_1.2.0_amd64.AppImage": elf}))
	check("an AppImage.tar.gz yields its AppImage", err == nil && bytes.Equal(got, elf), fmt.Sprint(err))
	_, err = update.ExtractAppImage(tarGz(map[string][]byte{"readme.txt": []byte("hi")}))
	check("a tar.gz without an AppImage is refused", errors.Is(err, update.ErrNoAppImage), fmt.Sprint(err))
	_, err = update.ExtractAppImage(tarGz(map[string][]byte{"fake.AppImage": []byte("not elf")}))
	check("a tar.gz whose AppImage is not an ELF image is refused", errors.Is(err, update.ErrNoAppImage), fmt.Sprint(err))
	_, err = update.ExtractAppImage([]byte("a macOS tarball, say"))
	check("a payload that is neither is refused", errors.Is(err, update.ErrNoAppImage), fmt.Sprint(err))
	_, err = update.ExtractAppImage(nil)
	check("an empty payload is refused", errors.Is(err, update.ErrNoAppImage), fmt.Sprint(err))

	target := filepath.Join(dir, "UniClipboard.AppImage")
	_ = os.WriteFile(target, []byte("old"), 0o700)
	err = update.InstallAppImage(tarGz(map[string][]byte{"x.AppImage": elf}), target)
	data, _ := os.ReadFile(target)
	info, _ := os.Stat(target)
	check("install replaces the file with the new image", err == nil && bytes.Equal(data, elf), fmt.Sprint(err))
	check("the replacement stays executable", err == nil && info.Mode().Perm()&0o100 != 0, fmt.Sprint(info.Mode()))
	leftovers, _ := filepath.Glob(filepath.Join(dir, ".uc-update-*"))
	check("no staging file is left behind", len(leftovers) == 0, fmt.Sprint(leftovers))

	err = update.InstallAppImage([]byte("garbage"), target)
	data, _ = os.ReadFile(target)
	check("a refused payload leaves the installed AppImage untouched", err != nil && bytes.Equal(data, elf), fmt.Sprint(err))
	err = update.InstallAppImage(elf, filepath.Join(dir, "missing.AppImage"))
	check("a missing target is refused", err != nil, fmt.Sprint(err))

	ro := filepath.Join(dir, "ro")
	_ = os.Mkdir(ro, 0o755)
	roTarget := filepath.Join(ro, "UniClipboard.AppImage")
	_ = os.WriteFile(roTarget, []byte("old"), 0o700)
	_ = os.Chmod(ro, 0o500)
	err = update.InstallAppImage(elf, roTarget)
	_ = os.Chmod(ro, 0o700)
	data, _ = os.ReadFile(roTarget)
	if os.Geteuid() == 0 {
		check("a read-only directory is refused (skipped: running as root)", true, "")
	} else {
		check("a read-only install directory is refused before anything changes", err != nil && string(data) == "old", fmt.Sprint(err))
	}
}
