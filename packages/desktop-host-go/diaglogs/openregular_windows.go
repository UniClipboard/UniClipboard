//go:build windows

package diaglogs

import (
	"errors"
	"os"
	"syscall"
)

const fileFlagOpenReparsePoint = 0x00200000

// openRegular opens path read-only without following a reparse point (symlink or junction): the handle is opened on
// the link itself and then refused unless it is a plain file.
func openRegular(path string) (*os.File, error) {
	name, err := syscall.UTF16PtrFromString(path)
	if err != nil {
		return nil, err
	}
	handle, err := syscall.CreateFile(name, syscall.GENERIC_READ,
		syscall.FILE_SHARE_READ|syscall.FILE_SHARE_WRITE|syscall.FILE_SHARE_DELETE, nil,
		syscall.OPEN_EXISTING, syscall.FILE_ATTRIBUTE_NORMAL|fileFlagOpenReparsePoint, 0)
	if err != nil {
		return nil, err
	}
	var data syscall.ByHandleFileInformation
	if err := syscall.GetFileInformationByHandle(handle, &data); err != nil {
		syscall.CloseHandle(handle)
		return nil, err
	}
	if data.FileAttributes&(syscall.FILE_ATTRIBUTE_REPARSE_POINT|syscall.FILE_ATTRIBUTE_DIRECTORY) != 0 {
		syscall.CloseHandle(handle)
		return nil, errors.New("not a regular file")
	}
	return os.NewFile(uintptr(handle), path), nil
}
