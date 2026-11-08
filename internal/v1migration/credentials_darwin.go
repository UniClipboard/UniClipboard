package v1migration

import (
	"bytes"
	"runtime"
	"unsafe"

	"github.com/ebitengine/purego"
)

// ReadSecret uses the same legacy generic-password API as keyring-rs and
// preserves arbitrary binary secrets, including zero bytes.
func ReadSecret(service, account string) ([]byte, error) {
	lib, err := purego.Dlopen("/System/Library/Frameworks/Security.framework/Security", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, ErrUnlock
	}
	defer func() { _ = purego.Dlclose(lib) }()
	var find func(uintptr, uint32, *byte, uint32, *byte, *uint32, *unsafe.Pointer, uintptr) int32
	var free func(uintptr, unsafe.Pointer) int32
	purego.RegisterLibFunc(&find, lib, "SecKeychainFindGenericPassword")
	purego.RegisterLibFunc(&free, lib, "SecKeychainItemFreeContent")
	var n uint32
	var ptr unsafe.Pointer
	sb, ab := append([]byte(service), 0), append([]byte(account), 0)
	status := find(0, uint32(len(service)), &sb[0], uint32(len(account)), &ab[0], &n, &ptr, 0)
	runtime.KeepAlive(sb)
	runtime.KeepAlive(ab)
	if status != 0 {
		return nil, ErrUnlock
	}
	defer free(0, ptr)
	if n > 1<<20 {
		return nil, ErrFormat
	}
	return bytes.Clone(unsafe.Slice((*byte)(ptr), int(n))), nil
}
