// Command update_stream exercises the production downloader and extractors with
// real HTTP, signatures and large files. Fixtures contain no user data.
package main

import (
	"archive/zip"
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"sync/atomic"
	"time"

	"aead.dev/minisign"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

func must(err error) {
	if err != nil {
		panic(err)
	}
}
func require(ok bool, message string) {
	if !ok {
		panic(message)
	}
}

func main() {
	if len(os.Args) != 2 {
		panic("usage: update_stream <outdir>")
	}
	out, err := filepath.Abs(os.Args[1])
	must(err)
	must(os.MkdirAll(out, 0o700))
	tmp := filepath.Join(out, "tmp")
	must(os.MkdirAll(tmp, 0o700))
	must(os.Setenv("TMPDIR", tmp))
	fixture, err := os.Create(filepath.Join(out, "fixture.AppImage"))
	must(err)
	defer fixture.Close()
	const size = 256 << 20
	_, err = fixture.Write([]byte("\x7fELF"))
	must(err)
	must(fixture.Truncate(size))
	pub, priv, err := minisign.GenerateKey(rand.Reader)
	must(err)
	sign := func() string {
		_, err := fixture.Seek(0, io.SeekStart)
		must(err)
		r := minisign.NewReader(fixture)
		_, err = io.Copy(io.Discard, r)
		must(err)
		return base64.StdEncoding.EncodeToString(r.Sign(priv))
	}
	signature := sign()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		file, err := os.Open(fixture.Name())
		if err != nil {
			http.Error(w, "fixture", 500)
			return
		}
		defer file.Close()
		if r.URL.Path == "/truncated" {
			w.Header().Set("Content-Length", fmt.Sprint(size+1))
		} else {
			w.Header().Set("Content-Length", fmt.Sprint(size))
		}
		if r.URL.Path == "/slow" {
			buf := make([]byte, 64<<10)
			for {
				n, err := file.Read(buf)
				if n > 0 {
					if _, err := w.Write(buf[:n]); err != nil {
						return
					}
					w.(http.Flusher).Flush()
				}
				if err != nil {
					return
				}
				select {
				case <-r.Context().Done():
					return
				case <-time.After(time.Millisecond):
				}
			}
		}
		_, _ = io.Copy(w, file)
	}))
	defer server.Close()
	client := update.Client{HTTP: server.Client(), PubKey: pub}
	rel := &update.Release{URL: server.URL + "/payload", Signature: signature}
	clean := func() {
		paths, err := filepath.Glob(filepath.Join(tmp, "uniclipboard-update-*"))
		must(err)
		require(len(paths) == 0, "leaked update staging")
	}
	runtime.GC()
	var before runtime.MemStats
	runtime.ReadMemStats(&before)
	var peak atomic.Uint64
	peak.Store(before.HeapAlloc)
	stop := make(chan struct{})
	sampled := make(chan struct{})
	go func() {
		defer close(sampled)
		for {
			var mem runtime.MemStats
			runtime.ReadMemStats(&mem)
			for old := peak.Load(); mem.HeapAlloc > old; old = peak.Load() {
				if peak.CompareAndSwap(old, mem.HeapAlloc) {
					break
				}
			}
			select {
			case <-stop:
				return
			case <-time.After(time.Millisecond):
			}
		}
	}()
	payload, err := client.Download(context.Background(), rel, nil)
	must(err)
	require(payload.Size == size, "wrong payload size")
	var path string
	must(payload.WithFile(func(f *os.File) error {
		path = f.Name()
		info, err := f.Stat()
		if err != nil {
			return err
		}
		require(info.Mode().Perm() == 0o600, "payload permissions")
		dir, err := os.Stat(filepath.Dir(path))
		if err != nil {
			return err
		}
		require(dir.Mode().Perm() == 0o700, "directory permissions")
		return client.VerifyFile(f, signature)
	}))
	target := filepath.Join(out, "installed.AppImage")
	must(os.WriteFile(target, []byte("old"), 0o700))
	must(payload.WithFile(func(f *os.File) error { return update.InstallAppImage(f, target) }))
	info, err := os.Stat(target)
	must(err)
	require(info.Size() == size, "installation truncated")
	require(info.Mode().Perm()&0o111 != 0, "installed image not executable")
	must(payload.Close())
	_, err = os.Stat(path)
	require(os.IsNotExist(err), "verified file retained after close")
	clean()
	close(stop)
	<-sampled
	heapGrowth := peak.Load() - before.HeapAlloc
	require(heapGrowth < 16<<20, fmt.Sprintf("large artifact used %d bytes of Go heap", heapGrowth))
	badPub, _, err := minisign.GenerateKey(rand.Reader)
	must(err)
	bad := client
	bad.PubKey = badPub
	_, err = bad.Download(context.Background(), rel, nil)
	require(err != nil, "untrusted signature accepted")
	clean()
	broken := *rel
	broken.URL = server.URL + "/truncated"
	_, err = client.Download(context.Background(), &broken, nil)
	require(err != nil, "truncated response accepted")
	clean()
	cancelCtx, cancel := context.WithCancel(context.Background())
	slow := *rel
	slow.URL = server.URL + "/slow"
	_, err = client.Download(cancelCtx, &slow, func(int, int64) { cancel() })
	require(errors.Is(err, context.Canceled), "cancel not propagated")
	clean()
	payload, err = client.Download(context.Background(), rel, nil)
	must(err)
	entered, release, used := make(chan struct{}), make(chan struct{}), make(chan error, 1)
	go func() { used <- payload.WithFile(func(*os.File) error { close(entered); <-release; return nil }) }()
	<-entered
	closed := make(chan error, 1)
	go func() { closed <- payload.Close() }()
	select {
	case <-closed:
		panic("cleanup raced active installer")
	case <-time.After(30 * time.Millisecond):
	}
	close(release)
	must(<-used)
	must(<-closed)
	clean()
	// Existing Ed signatures remain accepted without copying their message to the Go heap.
	legacy := []byte("\x7fELF legacy signed fixture")
	must(os.WriteFile(fixture.Name(), legacy, 0o600))
	legacySig := base64.StdEncoding.EncodeToString(minisign.Sign(priv, legacy))
	f, err := os.Open(fixture.Name())
	must(err)
	must(client.VerifyFile(f, legacySig))
	f.Close()
	// ZIP extraction uses the same bounded streaming copy for an NSIS installer.
	zipPath := filepath.Join(out, "installer.zip")
	zf, err := os.Create(zipPath)
	must(err)
	zw := zip.NewWriter(zf)
	entry, err := zw.Create("setup.exe")
	must(err)
	_, err = entry.Write([]byte("MZ"))
	must(err)
	_, err = io.CopyN(entry, zeroReader{}, 128<<20)
	must(err)
	must(zw.Close())
	must(zf.Close())
	zf, err = os.Open(zipPath)
	must(err)
	require(update.ExtractInstaller(zf, io.Discard) == nil, "ZIP installer extraction failed")
	zf.Close()
	results := map[string]any{"passed": true, "artifactBytes": size, "peakGoHeapGrowthBytes": heapGrowth, "untrustedRejected": true, "truncationRejected": true, "cancelCleaned": true, "verifiedFileCleaned": true, "installerPinnedFile": true, "legacySignatureAccepted": true, "zipExtractionPassed": true}
	data, err := json.MarshalIndent(results, "", "  ")
	must(err)
	must(os.WriteFile(filepath.Join(out, "result.json"), data, 0o600))
	fmt.Println(string(data))
	must(os.Remove(target))
	must(os.Remove(zipPath))
	must(os.Remove(fixture.Name()))
}

type zeroReader struct{}

func (zeroReader) Read(p []byte) (int, error) { clear(p); return len(p), nil }
