// Command signatureprobe is a headless E2E consumer of the production updater.
// It does not install packages or claim native coverage for explicit targets.
package main

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"time"

	"aead.dev/minisign"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

func main() {
	if len(os.Args) < 3 {
		fail(fmt.Errorf("usage: signatureprobe fixture-key <secret-dir> | consume <app.json> <feed-url>"))
	}
	switch os.Args[1] {
	case "fixture-key":
		dir := os.Args[2]
		check(os.MkdirAll(dir, 0o700))
		pub, private, err := minisign.GenerateKey(rand.Reader)
		check(err)
		encrypted, err := minisign.EncryptKey("disposable-e2e-password", private)
		check(err)
		check(os.WriteFile(filepath.Join(dir, "key"), encrypted, 0o600))
		text, err := pub.MarshalText()
		check(err)
		conf, err := json.Marshal(map[string]any{"updater": map[string]string{"pubkey": base64.StdEncoding.EncodeToString(text)}})
		check(err)
		check(os.WriteFile(filepath.Join(dir, "app.json"), conf, 0o600))
	case "consume":
		if len(os.Args) != 4 {
			fail(fmt.Errorf("consume requires config and feed URL"))
		}
		consume(os.Args[2], os.Args[3])
	default:
		fail(fmt.Errorf("unknown mode"))
	}
}

func consume(config, endpoint string) {
	data, err := os.ReadFile(config)
	check(err)
	var conf struct{ Updater struct{ Pubkey string } }
	check(json.Unmarshal(data, &conf))
	pub, err := update.ParsePublicKey(conf.Updater.Pubkey)
	check(err)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	check(err)
	httpClient := &http.Client{Timeout: 5 * time.Minute}
	resp, err := httpClient.Do(req)
	check(err)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		fail(fmt.Errorf("manifest HTTP status %d", resp.StatusCode))
	}
	var manifest update.Manifest
	check(json.NewDecoder(resp.Body).Decode(&manifest))
	if len(manifest.Platforms) == 0 {
		fail(fmt.Errorf("empty manifest"))
	}
	results := map[string]any{}
	for target := range manifest.Platforms {
		client := update.Client{HTTP: httpClient, PubKey: pub, Current: "0.0.0", Targets: []string{target},
			Endpoints: func(update.Channel) []string { return []string{endpoint} }}
		rel, err := client.Check(ctx, update.Stable)
		check(err)
		if rel == nil {
			fail(fmt.Errorf("no release for %s", target))
		}
		bytes, err := client.Download(ctx, rel, nil)
		check(err)
		hash := sha256.Sum256(bytes)
		if client.Verify(append(bytes, 1), rel.Signature) == nil {
			fail(fmt.Errorf("tampered payload accepted"))
		}
		results[target] = map[string]any{"url": rel.URL, "sha256": hex.EncodeToString(hash[:]), "verified": true, "tamperRejected": true}
	}
	out, err := json.MarshalIndent(results, "", "  ")
	check(err)
	fmt.Println(string(out))
}
func check(err error) {
	if err != nil {
		fail(err)
	}
}
func fail(err error) { fmt.Fprintln(os.Stderr, err); os.Exit(1) }
