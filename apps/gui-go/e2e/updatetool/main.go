// Command updatetool creates a throwaway minisign key pair and signs an update
// artifact for the update E2E: a valid signature, and a second one made by a
// different key to prove the app rejects untrusted artifacts.
package main

import (
	"crypto/rand"
	"encoding/base64"
	"fmt"
	"os"
	"path/filepath"

	"aead.dev/minisign"
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: updatetool <artifact> <outdir>")
		os.Exit(2)
	}
	artifact, err := os.ReadFile(os.Args[1])
	check(err)
	pub, priv, err := minisign.GenerateKey(rand.Reader)
	check(err)
	_, otherPriv, err := minisign.GenerateKey(rand.Reader)
	check(err)
	pubText, err := pub.MarshalText()
	check(err)
	out := os.Args[2]
	write := func(name string, data []byte) {
		check(os.WriteFile(filepath.Join(out, name), []byte(base64.StdEncoding.EncodeToString(data)), 0o600))
	}
	write("pubkey.b64", pubText)
	write("good.sig.b64", minisign.Sign(priv, artifact))
	write("bad.sig.b64", minisign.Sign(otherPriv, artifact))
}

func check(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
