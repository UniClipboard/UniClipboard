//go:build !windows

package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net"
	"os"
	"path/filepath"
)

func connect(profile, role string) (net.Conn, error) {
	sum := sha256.Sum256([]byte(filepath.Join(profile, "desktop-runtime")))
	return net.Dial("unix", filepath.Join("/tmp", fmt.Sprintf("unic-desktop-%d", os.Getuid()), hex.EncodeToString(sum[:16])+"-"+role+".sock"))
}
