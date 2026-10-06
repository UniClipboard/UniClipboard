// Command manifestprobe is the consumer side of the update-manifest E2E (slice 17c3). It never reimplements the
// manifest contract: `fixture` makes FIXTURE payloads with real minisign signatures, and `consume` drives the real
// internal/update client (DefaultTargets, Check, Download, Verify) against a feed produced by the real generator.
package main

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io/fs"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"aead.dev/minisign"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

func main() {
	if len(os.Args) < 2 {
		fail("usage: manifestprobe fixture <root> <pubkey-out> <relpath>... | consume <manifest> <root> <pubkey> <port> [installer]")
	}
	switch os.Args[1] {
	case "fixture":
		fixture(os.Args[2], os.Args[3], os.Args[4:])
	case "consume":
		installer := ""
		if len(os.Args) > 6 {
			installer = os.Args[6]
		}
		consume(os.Args[2], os.Args[3], os.Args[4], os.Args[5], installer)
	default:
		fail("unknown mode " + os.Args[1])
	}
}

// fixture writes a synthetic payload and its real minisign signature (base64, the Tauri `.sig` shape) per path.
// The payload bytes name the file so that a signature can only verify against its own architecture's payload.
func fixture(root, pubOut string, rels []string) {
	pub, priv, err := minisign.GenerateKey(rand.Reader)
	check(err)
	pubText, err := pub.MarshalText()
	check(err)
	check(os.WriteFile(pubOut, []byte(base64.StdEncoding.EncodeToString(pubText)), 0o600))
	for _, rel := range rels {
		full := filepath.Join(root, rel)
		check(os.MkdirAll(filepath.Dir(full), 0o755))
		payload := []byte("FIXTURE payload (synthetic, not a release asset): " + filepath.Base(rel) + "\n")
		check(os.WriteFile(full, payload, 0o644))
		sig := base64.StdEncoding.EncodeToString(minisign.Sign(priv, payload))
		check(os.WriteFile(full+".sig", []byte(sig), 0o644))
	}
}

type result struct {
	GOOS, GOARCH  string
	Targets       []string
	SelectedKey   string
	URL           string
	PayloadSHA256 string
	Error         string
	CrossVerify   map[string]string // other platform key -> "rejected" | "ACCEPTED"
}

func consume(manifestPath, root, pubPath, port, installer string) {
	encodedPub, err := os.ReadFile(pubPath)
	check(err)
	pub, err := update.ParsePublicKey(strings.TrimSpace(string(encodedPub)))
	check(err)
	manifest, err := os.ReadFile(manifestPath)
	check(err)
	// Artifacts are served by basename; a real release is flat, so a duplicate basename is a fixture error.
	byName := map[string]string{}
	check(filepath.WalkDir(root, func(p string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return err
		}
		if _, dup := byName[d.Name()]; dup {
			return fmt.Errorf("duplicate basename %s", d.Name())
		}
		byName[d.Name()] = p
		return nil
	}))
	mux := http.NewServeMux()
	mux.HandleFunc("/feed.json", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write(manifest)
	})
	mux.HandleFunc("/dl/", func(w http.ResponseWriter, r *http.Request) {
		p, ok := byName[filepath.Base(r.URL.Path)]
		if !ok {
			http.NotFound(w, r)
			return
		}
		http.ServeFile(w, r, p)
	})
	ln, err := net.Listen("tcp", "127.0.0.1:"+port)
	check(err)
	srv := &http.Server{Handler: mux}
	go func() { _ = srv.Serve(ln) }()
	defer srv.Close()

	feed := "http://127.0.0.1:" + port + "/feed.json"
	client := &update.Client{
		HTTP: update.NewHTTPClient(), PubKey: pub, Current: "1.0.0",
		Endpoints: func(update.Channel) []string { return []string{feed} },
		Targets:   update.DefaultTargets(installer),
	}
	res := result{GOOS: runtime.GOOS, GOARCH: runtime.GOARCH, Targets: client.Targets, CrossVerify: map[string]string{}}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	rel, err := client.Check(ctx, update.Stable)
	if err == nil && rel == nil {
		err = fmt.Errorf("no newer release")
	}
	if err != nil {
		res.Error = err.Error()
		emit(res)
		return
	}
	res.URL = rel.URL
	data, err := client.Download(ctx, rel, nil)
	if err != nil {
		res.Error = err.Error()
		emit(res)
		return
	}
	sum := sha256.Sum256(data)
	res.PayloadSHA256 = hex.EncodeToString(sum[:])
	var m update.Manifest
	check(json.Unmarshal(manifest, &m))
	for _, t := range client.Targets {
		if p, ok := m.Platforms[t]; ok && p.URL == rel.URL {
			res.SelectedKey = t
		}
	}
	// Negative control: every other platform's signature must NOT verify this payload.
	for key, p := range m.Platforms {
		if p.Signature == rel.Signature {
			continue
		}
		if client.Verify(data, p.Signature) == nil {
			res.CrossVerify[key] = "ACCEPTED"
		} else {
			res.CrossVerify[key] = "rejected"
		}
	}
	emit(res)
}

func emit(r result) {
	out, _ := json.MarshalIndent(r, "", "  ")
	fmt.Println(string(out))
}

func check(err error) {
	if err != nil {
		fail(err.Error())
	}
}

func fail(msg string) {
	fmt.Fprintln(os.Stderr, msg)
	os.Exit(1)
}
