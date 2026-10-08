// Command updater-sign signs update archives with the existing Tauri/minisign
// key. Secrets are read from environment only and are never printed or written.
package main

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"aead.dev/minisign"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

type record struct {
	Filename        string `json:"filename"`
	SHA256          string `json:"sha256"`
	SignatureSHA256 string `json:"signatureSha256"`
	Verified        bool   `json:"verifiedByClient"`
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	config := flag.String("app-config", "app.json", "app identity containing the updater public key")
	directory := flag.String("artifacts-dir", "", "flat directory of named update archives")
	evidence := flag.String("evidence", "", "optional public verification evidence JSON")
	verify := flag.Bool("verify-only", false, "verify existing sidecars without loading any secret")
	flag.Parse()
	if *directory == "" || flag.NArg() != 0 {
		return errors.New("required: --artifacts-dir <directory>")
	}
	conf, err := os.ReadFile(*config)
	if err != nil {
		return err
	}
	var app struct {
		Updater struct {
			Pubkey string `json:"pubkey"`
		} `json:"updater"`
	}
	if err := json.Unmarshal(conf, &app); err != nil {
		return errors.New("invalid app config")
	}
	pub, err := update.ParsePublicKey(app.Updater.Pubkey)
	if err != nil {
		return errors.New("invalid updater public key in app config")
	}
	client := update.Client{PubKey: pub}
	var private minisign.PrivateKey
	if !*verify {
		private, err = loadKey()
		if err != nil {
			return err
		}
		// Validate the complete key, not just its key ID, before writing any sidecar.
		if client.Verify([]byte("updater key compatibility"), base64.StdEncoding.EncodeToString(minisign.Sign(private, []byte("updater key compatibility")))) != nil {
			return errors.New("updater secret does not match app config public key")
		}
	}
	entries, err := os.ReadDir(*directory)
	if err != nil {
		return err
	}
	records := []record{}
	for _, entry := range entries {
		name := entry.Name()
		if entry.IsDir() || !(strings.HasSuffix(name, ".app.tar.gz") || strings.HasSuffix(name, ".AppImage.tar.gz") || strings.HasSuffix(name, "-setup.exe")) {
			continue
		}
		if !strings.HasPrefix(name, "UniClipboard") {
			return fmt.Errorf("unexpected updater artifact name: %s", name)
		}
		file := filepath.Join(*directory, name)
		info, err := os.Lstat(file)
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			return fmt.Errorf("updater artifact must be a regular file: %s", name)
		}
		data, err := os.ReadFile(file)
		if err != nil {
			return err
		}
		if len(data) == 0 {
			return fmt.Errorf("empty updater artifact: %s", name)
		}
		var sig []byte
		if *verify {
			sig, err = os.ReadFile(file + ".sig")
			if err != nil {
				return err
			}
		} else {
			sig = []byte(base64.StdEncoding.EncodeToString(minisign.Sign(private, data)))
		}
		if err := client.Verify(data, string(sig)); err != nil {
			return fmt.Errorf("client verification failed: %s", name)
		}
		if !*verify {
			if err := os.WriteFile(file+".sig", append(sig, '\n'), 0o644); err != nil {
				return err
			}
		}
		sum := sha256.Sum256(data)
		sidecar, err := os.ReadFile(file + ".sig")
		if err != nil {
			return err
		}
		sigSum := sha256.Sum256(sidecar)
		records = append(records, record{name, hex.EncodeToString(sum[:]), hex.EncodeToString(sigSum[:]), true})
	}
	if len(records) == 0 {
		return errors.New("no named updater artifacts found")
	}
	output, err := json.MarshalIndent(struct {
		PublicKey string   `json:"publicKey"`
		Artifacts []record `json:"artifacts"`
	}{app.Updater.Pubkey, records}, "", "  ")
	if err != nil {
		return err
	}
	output = append(output, '\n')
	if *evidence != "" {
		if err := os.WriteFile(*evidence, output, 0o644); err != nil {
			return err
		}
	}
	fmt.Print(string(output))
	return nil
}

func loadKey() (minisign.PrivateKey, error) {
	encoded := strings.TrimSpace(os.Getenv("TAURI_SIGNING_PRIVATE_KEY"))
	password := os.Getenv("TAURI_SIGNING_PRIVATE_KEY_PASSWORD")
	// Do not pass secrets to child processes spawned by this command in the future.
	_ = os.Unsetenv("TAURI_SIGNING_PRIVATE_KEY")
	_ = os.Unsetenv("TAURI_SIGNING_PRIVATE_KEY_PASSWORD")
	if encoded == "" {
		return minisign.PrivateKey{}, errors.New("TAURI_SIGNING_PRIVATE_KEY is unavailable")
	}
	text := []byte(encoded)
	if !strings.HasPrefix(encoded, "untrusted comment:") {
		var err error
		text, err = base64.StdEncoding.DecodeString(encoded)
		if err != nil {
			return minisign.PrivateKey{}, errors.New("invalid updater secret encoding")
		}
	}
	var key minisign.PrivateKey
	var err error
	if minisign.IsEncrypted(text) {
		key, err = minisign.DecryptKey(password, text)
	} else {
		err = key.UnmarshalText(text)
	}
	if err != nil {
		return minisign.PrivateKey{}, errors.New("cannot decrypt or parse updater secret")
	}
	return key, nil
}
