// Command update-sign manages the raw Ed25519 archive signatures used by mygo.
package main

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func run() error {
	keygen := flag.String("keygen", "", "Create a private directory containing public.key and private.key")
	verify := flag.Bool("verify", false, "Verify a signature with a public key")
	publicKey := flag.String("public-key", "", "Raw base64 public key")
	signature := flag.String("signature", "", "Base64 archive signature")
	file := flag.String("file", "", "Archive to sign")
	flag.Parse()
	if *keygen != "" {
		if err := os.Mkdir(*keygen, 0700); err != nil {
			return err
		}
		pub, priv, err := ed25519.GenerateKey(rand.Reader)
		if err != nil {
			return err
		}
		for name, value := range map[string][]byte{"public.key": pub, "private.key": priv} {
			if err := os.WriteFile(filepath.Join(*keygen, name), []byte(base64.StdEncoding.EncodeToString(value)+"\n"), 0600); err != nil {
				return err
			}
		}
		return nil
	}
	if *verify {
		key, err := base64.StdEncoding.DecodeString(*publicKey)
		if err != nil || len(key) != ed25519.PublicKeySize {
			return fmt.Errorf("invalid public key")
		}
		sig, err := base64.StdEncoding.DecodeString(*signature)
		if err != nil {
			return err
		}
		f, err := os.Open(*file)
		if err != nil {
			return err
		}
		defer func() { _ = f.Close() }()
		h := sha256.New()
		if _, err = io.Copy(h, f); err != nil {
			return err
		}
		if !ed25519.Verify(key, h.Sum(nil), sig) {
			return fmt.Errorf("archive signature mismatch")
		}
		return nil
	}
	raw, err := base64.StdEncoding.DecodeString(strings.TrimSpace(os.Getenv("UNIC_UPDATE_PRIVATE_KEY")))
	if err != nil || len(raw) != ed25519.PrivateKeySize {
		return fmt.Errorf("UNIC_UPDATE_PRIVATE_KEY must be a raw base64 Ed25519 private key")
	}
	key := ed25519.PrivateKey(raw)
	// Reject inconsistent seed/public halves instead of producing unverifiable signatures.
	if !key.Equal(ed25519.NewKeyFromSeed(key.Seed())) {
		return fmt.Errorf("inconsistent Ed25519 private key")
	}
	f, err := os.Open(*file)
	if err != nil {
		return err
	}
	defer func() { _ = f.Close() }()
	h := sha256.New()
	n, err := io.Copy(h, f)
	if err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(map[string]any{"size": n, "sha256": hex.EncodeToString(h.Sum(nil)), "signature": base64.StdEncoding.EncodeToString(ed25519.Sign(key, h.Sum(nil))), "public_key": base64.StdEncoding.EncodeToString(key.Public().(ed25519.PublicKey))})
}
