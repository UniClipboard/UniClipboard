package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"net"
	"path/filepath"
	"strings"

	winio "github.com/tailscale/go-winio"
)

func connect(profile, role string) (net.Conn, error) {
	sum := sha256.Sum256([]byte(strings.ToLower(filepath.Join(profile, "desktop-runtime"))))
	return winio.DialPipeContext(context.Background(), `\\.\pipe\unic-desktop-`+hex.EncodeToString(sum[:16])+"-"+role)
}
