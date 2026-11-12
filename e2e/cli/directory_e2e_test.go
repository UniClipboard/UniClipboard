package e2e

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/rogpeppe/go-internal/testscript"
)

// treeInventory includes empty directories, file sizes and exact byte hashes.
// It never follows links. It is the process scenario's independent oracle.
func treeInventory(root string) ([]string, error) {
	var out []string
	err := filepath.WalkDir(root, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(root, p)
		if err != nil {
			return err
		}
		rel = filepath.ToSlash(rel)
		if d.IsDir() {
			out = append(out, "D "+rel)
			return nil
		}
		if !d.Type().IsRegular() {
			return fmt.Errorf("unexpected special entry")
		}
		b, err := os.ReadFile(p)
		if err != nil {
			return err
		}
		h := sha256.Sum256(b)
		out = append(out, fmt.Sprintf("F %s %d %s", rel, len(b), hex.EncodeToString(h[:])))
		return nil
	})
	sort.Strings(out)
	return out, err
}

func cmdTreeWait(ts *testscript.TestScript, neg bool, args []string) {
	if len(args) != 2 {
		ts.Fatalf("usage: tree-wait node expected-root")
	}
	st := stateOf(ts)
	expected := ts.MkAbs(args[1])
	want, err := treeInventory(expected)
	ts.Check(err)
	deadline := time.Now().Add(20 * time.Second)
	if neg {
		deadline = time.Now().Add(2 * time.Second)
	}
	var got []string
	for time.Now().Before(deadline) {
		b, e := os.ReadFile(filepath.Join(st.nodes[args[0]].home, "board", "files"))
		if e == nil {
			for _, p := range strings.Split(strings.TrimSpace(string(b)), "\n") {
				if filepath.Base(p) != filepath.Base(expected) {
					continue
				}
				got, e = treeInventory(p)
				if e == nil && reflect.DeepEqual(want, got) {
					if neg {
						ts.Fatalf("unexpected directory delivered")
					}
					if dir := os.Getenv("UNIC_E2E_ARTIFACTS"); dir != "" {
						ts.Check(os.MkdirAll(dir, 0700))
						receipt := map[string]any{"scenario": strings.TrimPrefix(filepath.Base(st.work), "script-"), "assertion": "directory tree bytes", "result": "PASS", "expected": want, "received": got, "time": time.Now().UTC()}
						data, e := json.MarshalIndent(receipt, "", "  ")
						ts.Check(e)
						ts.Check(os.WriteFile(filepath.Join(dir, filepath.Base(st.work)+"-"+strings.ReplaceAll(filepath.ToSlash(args[1]), "/", "__")+".json"), data, 0600))
					}
					return
				}
			}
		}
		time.Sleep(100 * time.Millisecond)
	}
	if !neg {
		for _, command := range [][]string{{"history", "list", "--json"}, {"recv", "list", "--json"}} {
			out, stderr, err := st.nodes[args[0]].unic(command...)
			ts.Logf("diagnostic %s %s %v", out, stderr, err)
		}
		ts.Fatalf("directory tree mismatch: expected %v; received %v", want, got)
	}
}

func directoryRequest(ts *testscript.TestScript, node, method, path, body string) []byte {
	n := stateOf(ts).node(ts, node)
	client := http.Client{Timeout: 5 * time.Second, Transport: &http.Transport{DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
		return (&net.Dialer{}).DialContext(ctx, "unix", n.sock)
	}}}
	req, err := http.NewRequest(method, "http://unic"+path, strings.NewReader(body))
	ts.Check(err)
	resp, err := client.Do(req)
	ts.Check(err)
	defer func() { _ = resp.Body.Close() }()
	data, err := io.ReadAll(resp.Body)
	ts.Check(err)
	if resp.StatusCode != 200 {
		ts.Fatalf("directory request status %d: %s", resp.StatusCode, data)
	}
	return data
}
func cmdDirectoryProbe(ts *testscript.TestScript, neg bool, args []string) {
	if len(args) != 3 || neg {
		ts.Fatalf("usage: directory-probe node scans snapshots")
	}
	scans, err := strconv.ParseUint(args[1], 10, 64)
	ts.Check(err)
	snapshots, err := strconv.ParseUint(args[2], 10, 64)
	ts.Check(err)
	end := time.Now().Add(5 * time.Second)
	for {
		var v struct{ Scans, Snapshots uint64 }
		ts.Check(json.Unmarshal(directoryRequest(ts, args[0], "GET", "/test/directories", ""), &v))
		if v.Scans == scans && v.Snapshots == snapshots {
			return
		}
		if time.Now().After(end) {
			ts.Fatalf("directory work counters: got %d/%d want %d/%d", v.Scans, v.Snapshots, scans, snapshots)
		}
		time.Sleep(50 * time.Millisecond)
	}
}
func cmdDirectoryHold(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) != 2 {
		ts.Fatalf("usage: directory-hold node true|false")
	}
	body := `{"Hold":` + args[1] + `}`
	if args[1] != "true" && args[1] != "false" {
		body = `{"Hold":true,"Stage":"` + args[1] + `"}`
	}
	directoryRequest(ts, args[0], "PUT", "/test/directories", body)
}
func cmdSubmissionCancel(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) != 2 {
		ts.Fatalf("usage: submission-cancel node request-id")
	}
	directoryRequest(ts, args[0], "DELETE", "/v1/submissions/"+args[1], "")
}

func cmdDirectoryAttack(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) != 2 {
		ts.Fatalf("usage: directory-attack node mode")
	}
	directoryRequest(ts, args[0], "POST", "/test/directory-attack", `{"Mode":"`+args[1]+`"}`)
}

func cmdBoardSelect(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) < 2 {
		ts.Fatalf("usage: board-select node path...")
	}
	n := stateOf(ts).node(ts, args[0])
	dir := filepath.Join(n.home, "board")
	var paths []string
	for _, p := range args[1:] {
		paths = append(paths, ts.MkAbs(p))
	}
	for _, name := range []string{"text", "html", "png"} {
		_ = os.Remove(filepath.Join(dir, name))
	}
	tmp := filepath.Join(dir, ".selection")
	ts.Check(os.WriteFile(tmp, []byte(strings.Join(paths, "\n")+"\n"), 0600))
	ts.Check(os.Rename(tmp, filepath.Join(dir, "files")))
}
func cmdGenTree(ts *testscript.TestScript, _ bool, args []string) {
	if len(args) != 3 {
		ts.Fatalf("usage: gen-tree path depth files")
	}
	root := ts.MkAbs(args[0])
	depth, err := strconv.Atoi(args[1])
	ts.Check(err)
	count, err := strconv.Atoi(args[2])
	ts.Check(err)
	ts.Check(os.MkdirAll(root, 0700))
	path := root
	for i := 0; i < depth; i++ {
		path = filepath.Join(path, "d")
		ts.Check(os.Mkdir(path, 0700))
	}
	for i := 0; i < count; i++ {
		ts.Check(os.WriteFile(filepath.Join(root, fmt.Sprintf("f%05d", i)), []byte("fixture"), 0600))
	}
}

func cmdDirectoryStaging(ts *testscript.TestScript, neg bool, args []string) {
	if len(args) != 1 {
		ts.Fatalf("usage: directory-staging node")
	}
	n := stateOf(ts).node(ts, args[0])
	entries, err := os.ReadDir(filepath.Join(n.home, "inbox"))
	if err != nil && !os.IsNotExist(err) {
		ts.Check(err)
	}
	found := false
	for _, e := range entries {
		found = found || strings.HasPrefix(e.Name(), "unic-content-")
	}
	if found == neg {
		ts.Fatalf("directory staging existence=%v", found)
	}
}
