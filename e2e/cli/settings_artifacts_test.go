package e2e

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"time"

	"github.com/rogpeppe/go-internal/testscript"
	_ "modernc.org/sqlite"
)

func cmdFilePath(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) != 2 {
		ts.Fatalf("usage: file-path node variable")
	}
	n := stateOf(ts).node(ts, args[0])
	data, err := os.ReadFile(filepath.Join(n.home, "board", "files"))
	ts.Check(err)
	path := strings.TrimSpace(strings.Split(string(data), "\n")[0])
	if !filepath.IsAbs(path) {
		ts.Fatalf("missing file path")
	}
	ts.Setenv(args[1], path)
}
func cmdHistoryAge(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) != 2 {
		ts.Fatalf("usage: history-age stopped-node seconds")
	}
	n := stateOf(ts).node(ts, args[0])
	select {
	case <-n.done:
	default:
		ts.Fatalf("history-age requires a stopped disposable node")
	}
	seconds, err := strconv.ParseInt(args[1], 10, 64)
	ts.Check(err)
	db, err := sql.Open("sqlite", filepath.Join(n.home, "unic.db"))
	ts.Check(err)
	defer func() { _ = db.Close() }()
	_, err = db.Exec(`UPDATE clip_items SET created_ms=?`, time.Now().Add(-time.Duration(seconds)*time.Second).UnixMilli())
	ts.Check(err)
}
func digest(data []byte) string { sum := sha256.Sum256(data); return hex.EncodeToString(sum[:]) }

// exportArtifact retains only fixture configs, file hashes, and test identities.
// It never copies node databases, pairing tickets, or daemon logs.
func exportArtifact(env *testscript.Env, st *state) {
	dest := os.Getenv("UNIC_E2E_ARTIFACTS")
	if dest == "" {
		return
	}
	dest = filepath.Join(dest, filepath.Base(env.WorkDir))
	if err := os.MkdirAll(dest, 0700); err != nil {
		env.T().Fatal(err)
	}
	hashes := map[string]string{}
	for name, n := range st.nodes {
		if data, err := os.ReadFile(filepath.Join(n.home, "config.toml")); err == nil {
			if err = os.WriteFile(filepath.Join(dest, name+".config.toml"), data, 0600); err != nil {
				env.T().Fatal(err)
			}
		}
	}
	_ = filepath.WalkDir(env.WorkDir, func(path string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return nil
		}
		if d.Type()&os.ModeSymlink != 0 {
			return nil
		}
		rel, _ := filepath.Rel(env.WorkDir, path)
		if strings.HasSuffix(rel, ".bin") || strings.HasSuffix(rel, ".txt") {
			if data, err := os.ReadFile(path); err == nil {
				hashes[rel] = digest(data)
			}
		}
		return nil
	})
	binaries := map[string]string{}
	for _, name := range []string{"unic", "unicd"} {
		if data, err := os.ReadFile(filepath.Join(binDir, name)); err == nil {
			binaries[name] = digest(data)
		}
	}
	root, rootErr := repoRoot()
	if rootErr != nil {
		env.T().Fatal(rootErr)
	}
	cmd := exec.Command("git", "rev-parse", "HEAD")
	cmd.Dir = root
	source, _ := cmd.Output()
	cmd = exec.Command("git", "diff", "--", "engine", "internal", "plugin", "app/service", "e2e/cli")
	cmd.Dir = root
	patch, _ := cmd.Output()
	sourceHashes := map[string]string{}
	for _, base := range []string{"engine", "internal", "plugin", "app/service", "e2e/cli"} {
		_ = filepath.WalkDir(filepath.Join(root, base), func(path string, d fs.DirEntry, err error) error {
			if err != nil || d.IsDir() {
				return nil
			}
			if strings.HasSuffix(path, ".go") || strings.HasSuffix(path, ".txtar") {
				data, err := os.ReadFile(path)
				if err == nil {
					rel, _ := filepath.Rel(root, path)
					sourceHashes[rel] = digest(data)
				}
			}
			return nil
		})
	}

	passed := true
	if f, ok := env.T().(interface{ Failed() bool }); ok {
		passed = !f.Failed()
	}
	manifest := map[string]any{"source_sha": strings.TrimSpace(string(source)), "tracked_patch_sha256": digest(patch), "source_files_sha256": sourceHashes, "binaries_sha256": binaries, "fixture_sha256": hashes, "go": runtime.Version(), "os": runtime.GOOS, "arch": runtime.GOARCH, "passed": passed, "completed_at": time.Now().UTC().Format(time.RFC3339)}
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		env.T().Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(dest, "manifest.json"), data, 0600); err != nil {
		env.T().Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(dest, "replay.txt"), []byte(fmt.Sprintf("UNIC_E2E=1 UNIC_E2E_BIN=<matching-binary-directory> go test ./e2e/cli -run TestCLI/%s -count=1 -v\n", strings.TrimPrefix(filepath.Base(env.WorkDir), "script-"))), 0600); err != nil {
		env.T().Fatal(err)
	}
}

// file-wait waits for a physical deletion instead of assuming the poller has run.
func cmdFileWait(ts *testscript.TestScript, neg bool, args []string) {
	if len(args) != 1 {
		ts.Fatalf("usage: [!] file-wait path")
	}
	deadline := time.Now().Add(10 * time.Second)
	for {
		_, err := os.Stat(args[0])
		exists := err == nil
		if exists != neg {
			return
		}
		if time.Now().After(deadline) {
			ts.Fatalf("file presence did not reach expected state")
		}
		time.Sleep(50 * time.Millisecond)
	}
}

func cmdCacheLink(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) != 2 {
		ts.Fatalf("usage: cache-link stopped-node cached-file")
	}
	st := stateOf(ts)
	n := st.node(ts, args[0])
	select {
	case <-n.done:
	default:
		ts.Fatalf("cache-link requires stopped node")
	}
	target := filepath.Dir(args[1])
	expected := filepath.Join(n.home, "inbox", "file-cache")
	if filepath.Dir(target) != expected {
		ts.Fatalf("not a default cache payload")
	}
	ts.Check(os.Rename(target, filepath.Join(st.work, "moved-payload")))
	ts.Check(os.Symlink(filepath.Join(st.work, "outside"), target))
}
func cmdConfigConflict(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) != 1 {
		ts.Fatalf("usage: config-conflict node")
	}
	n := stateOf(ts).node(ts, args[0])
	client := http.Client{Transport: &http.Transport{DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
		var d net.Dialer
		return d.DialContext(ctx, "unix", n.sock)
	}}}
	resp, err := client.Get("http://unic/v1/config")
	ts.Check(err)
	var state struct{ Revision string }
	ts.Check(json.NewDecoder(resp.Body).Decode(&state))
	_ = resp.Body.Close()
	raw, err := json.Marshal(map[string]string{"revision": state.Revision, "toml": "[sync]\nautomatic = false\n"})
	ts.Check(err)
	for _, want := range []int{200, 409} {
		req, err := http.NewRequest("PUT", "http://unic/v1/config", bytes.NewReader(raw))
		ts.Check(err)
		resp, err := client.Do(req)
		ts.Check(err)
		_, _ = io.Copy(io.Discard, resp.Body)
		_ = resp.Body.Close()
		if resp.StatusCode != want {
			ts.Fatalf("config status %d, want %d", resp.StatusCode, want)
		}
	}
}

// cmdExternalRef checks the persisted ownership class, not an Engine implementation detail.
func cmdExternalRef(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) != 1 {
		ts.Fatalf("usage: external-ref node")
	}
	n := stateOf(ts).node(ts, args[0])
	db, err := sql.Open("sqlite", filepath.Join(n.home, "unic.db"))
	ts.Check(err)
	defer func() { _ = db.Close() }()
	var ref, blob, version string
	ts.Check(db.QueryRow(`SELECT ref,blob,version FROM clip_item_parts WHERE kind='file' ORDER BY item_seq DESC,ord LIMIT 1`).Scan(&ref, &blob, &version))
	if !strings.HasPrefix(ref, "desktop-received:") || filepath.IsAbs(ref) || blob != "" || version == "" {
		ts.Fatalf("custom receive must use an opaque host reference without an owned blob")
	}
	ts.Logf("host reference ownership verified; reference is not a filesystem path")
}
