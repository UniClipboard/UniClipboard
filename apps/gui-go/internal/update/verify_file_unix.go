//go:build !windows

package update

import (
	"aead.dev/minisign"
	"golang.org/x/sys/unix"
	"os"
)

// Legacy Ed signatures cover the raw message and cannot use minisign.Reader.
// Map only for verification and immediately unmap. Remove this compatibility
// path once all supported release feeds carry HashEdDSA sidecars from updater-sign.
func verifyLegacyFile(file *os.File, size int64, key minisign.PublicKey, signature []byte) (bool, error) {
	if size == 0 {
		return minisign.Verify(key, nil, signature), nil
	}
	data, err := unix.Mmap(int(file.Fd()), 0, int(size), unix.PROT_READ, unix.MAP_PRIVATE)
	if err != nil {
		return false, err
	}
	valid := minisign.Verify(key, data, signature)
	return valid, unix.Munmap(data)
}
