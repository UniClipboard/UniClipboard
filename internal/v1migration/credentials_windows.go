package v1migration

import (
	"bytes"
	"context"
	"errors"
	"unsafe"

	"golang.org/x/sys/windows"
)

func ReadSecret(service, account string) ([]byte, error) {
	dll := windows.NewLazySystemDLL("advapi32.dll")
	read, free := dll.NewProc("CredReadW"), dll.NewProc("CredFree")
	name, err := windows.UTF16PtrFromString(account + "." + service)
	if err != nil {
		return nil, credentialError("Windows credential lookup parameters are invalid; select the original v1 profile")
	}
	type credential struct {
		Flags, Type             uint32
		TargetName, Comment     *uint16
		LastWritten             windows.Filetime
		Size                    uint32
		Blob                    *byte
		Persist, AttributeCount uint32
		Attributes              uintptr
		Alias, UserName         *uint16
	}
	var c *credential
	ok, _, callErr := read.Call(uintptr(unsafe.Pointer(name)), 1, 0, uintptr(unsafe.Pointer(&c)))
	if ok == 0 || c == nil {
		if errors.Is(callErr, windows.ERROR_NOT_FOUND) {
			return nil, credentialError("Windows credential was not found; use the original Windows account and v1 credential service")
		}
		return nil, credentialError("Windows Credential Manager access failed; sign in with the original Windows account and allow credential access")
	}
	defer func() { _, _, _ = free.Call(uintptr(unsafe.Pointer(c))) }()
	if c.Size > 1<<20 || c.Size > 0 && c.Blob == nil {
		return nil, ErrFormat
	}
	return bytes.Clone(unsafe.Slice(c.Blob, int(c.Size))), nil
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
