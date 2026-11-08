package v1migration

import (
	"bytes"
	"unsafe"

	"golang.org/x/sys/windows"
)

func ReadSecret(service, account string) ([]byte, error) {
	dll := windows.NewLazySystemDLL("advapi32.dll")
	read, free := dll.NewProc("CredReadW"), dll.NewProc("CredFree")
	name, err := windows.UTF16PtrFromString(account + "." + service)
	if err != nil {
		return nil, ErrUnlock
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
	ok, _, _ := read.Call(uintptr(unsafe.Pointer(name)), 1, 0, uintptr(unsafe.Pointer(&c)))
	if ok == 0 || c == nil {
		return nil, ErrUnlock
	}
	defer func() { _, _, _ = free.Call(uintptr(unsafe.Pointer(c))) }()
	if c.Size > 1<<20 {
		return nil, ErrFormat
	}
	return bytes.Clone(unsafe.Slice(c.Blob, int(c.Size))), nil
}
