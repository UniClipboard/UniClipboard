//go:build windows

package update

import (
	"aead.dev/minisign"
	"github.com/edsrzf/mmap-go"
	"os"
)

// Keep legacy raw-message signatures readable until supported feeds have all
// migrated to the HashEdDSA sidecars produced by updater-sign.
func verifyLegacyFile(file *os.File, size int64, key minisign.PublicKey, signature []byte) (bool, error) {
	if size == 0 {
		return minisign.Verify(key, nil, signature), nil
	}
	data, err := mmap.MapRegion(file, int(size), mmap.RDONLY, 0, 0)
	if err != nil {
		return false, err
	}
	valid := minisign.Verify(key, data, signature)
	return valid, data.Unmap()
}
