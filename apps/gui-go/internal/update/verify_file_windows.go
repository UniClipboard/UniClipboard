//go:build windows

package update

import (
	"aead.dev/minisign"
	"golang.org/x/sys/windows"
	"os"
	"unsafe"
)

// Keep legacy raw-message signatures readable until supported feeds have all
// migrated to the HashEdDSA sidecars produced by updater-sign.
func verifyLegacyFile(file *os.File, size int64, key minisign.PublicKey, signature []byte) (bool, error) {
	if size == 0 {
		return minisign.Verify(key, nil, signature), nil
	}
	mapping, err := windows.CreateFileMapping(windows.Handle(file.Fd()), nil, windows.PAGE_READONLY, 0, 0, nil)
	if err != nil {
		return false, err
	}
	defer windows.CloseHandle(mapping)
	address, err := windows.MapViewOfFile(mapping, windows.FILE_MAP_READ, 0, 0, uintptr(size))
	if err != nil {
		return false, err
	}
	valid := minisign.Verify(key, unsafe.Slice((*byte)(unsafe.Pointer(address)), int(size)), signature)
	return valid, windows.UnmapViewOfFile(address)
}
