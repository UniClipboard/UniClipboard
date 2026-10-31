// Command legacy_signature verifies legacy updater signatures through real HTTP
// downloads and file mappings without installing or launching an application.
package main

import (
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"

	"aead.dev/minisign"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

func main() {
	if len(os.Args) != 2 {
		panic("usage: legacy_signature <evidence-directory>")
	}
	out := os.Args[1]
	if err := os.MkdirAll(out, 0o700); err != nil {
		panic(err)
	}
	pub, private, err := minisign.GenerateKey(rand.Reader)
	if err != nil {
		panic(err)
	}
	checks := []string{}
	for _, size := range []int{0, 8 << 20} {
		data := make([]byte, size)
		if _, err := rand.Read(data); err != nil {
			panic(err)
		}
		signature := base64.StdEncoding.EncodeToString(minisign.Sign(private, data))
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			_, _ = w.Write(data)
		}))
		client := update.Client{HTTP: server.Client(), PubKey: pub}
		payload, err := client.Download(context.Background(), &update.Release{URL: server.URL, Signature: signature}, nil)
		if err != nil {
			server.Close()
			panic(err)
		}
		err = payload.WithFile(func(file *os.File) error { return client.VerifyFile(file, signature) })
		if closeErr := payload.Close(); err == nil {
			err = closeErr
		}
		if err != nil {
			server.Close()
			panic(err)
		}
		other, _, err := minisign.GenerateKey(rand.Reader)
		if err != nil {
			panic(err)
		}
		client.PubKey = other
		rejected, err := client.Download(context.Background(), &update.Release{URL: server.URL, Signature: signature}, nil)
		server.Close()
		if rejected != nil {
			_ = rejected.Close()
		}
		if err == nil {
			panic("legacy signature accepted with an untrusted key")
		}
		checks = append(checks, fmt.Sprintf("legacy %d-byte HTTP download, file verification, cleanup and untrusted-key rejection", size))
	}
	receipt, err := json.MarshalIndent(map[string]any{"passed": true, "checks": checks}, "", "  ")
	if err != nil {
		panic(err)
	}
	if err := os.WriteFile(filepath.Join(out, "receipt.json"), receipt, 0o600); err != nil {
		panic(err)
	}
	fmt.Println(string(receipt))
}
