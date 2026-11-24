//go:build darwin && !ios

package v1migration

import (
	"bytes"
	"context"
	"runtime"
	"unsafe"

	"github.com/ebitengine/purego"
)

// ReadSecret uses the same legacy generic-password API as keyring-rs and
// preserves arbitrary binary secrets, including zero bytes.
func ReadSecret(service, account string) ([]byte, error) {
	lib, err := purego.Dlopen("/System/Library/Frameworks/Security.framework/Security", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, credentialError("macOS Keychain is unavailable; unlock the login keychain")
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
		if status == -25300 { // errSecItemNotFound
			return nil, credentialError("macOS credential was not found; select the original v1 profile and credential service")
		}
		if status == -128 { // userCanceledErr
			return nil, credentialError("macOS Keychain prompt was cancelled; retry and allow access")
		}
		return nil, credentialError("macOS Keychain access failed; unlock the login keychain and allow credential access")
	}
	defer free(0, ptr)
	if n > 1<<20 {
		return nil, ErrFormat
	}
	return bytes.Clone(unsafe.Slice((*byte)(ptr), int(n))), nil
}

// ReadSecretContext checks cancellation around the synchronous native API.
// The OS owns any native authorization prompt; the caller cannot dismiss it.
func ReadSecretContext(ctx context.Context, service, account string) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	value, err := ReadSecret(service, account)
	if cancelErr := ctx.Err(); cancelErr != nil {
		clear(value)
		return nil, cancelErr
	}
	return value, err
}
