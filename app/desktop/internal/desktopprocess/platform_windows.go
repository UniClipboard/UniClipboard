package desktopprocess

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"

	winio "github.com/tailscale/go-winio"
	"golang.org/x/sys/windows"
)

func samePath(a, b string) bool { return strings.EqualFold(a, b) }
func userSID() (string, error) {
	token := windows.GetCurrentProcessToken()
	user, err := token.GetTokenUser()
	if err != nil {
		return "", err
	}
	return user.User.Sid.String(), nil
}
func privateDir(path string) error {
	if err := os.Mkdir(path, 0700); err != nil && !errors.Is(err, os.ErrExist) {
		return err
	}
	stat, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !stat.IsDir() || stat.Mode()&os.ModeSymlink != 0 {
		return errors.New("desktop runtime directory must not be a symlink")
	}
	sid, err := userSID()
	if err != nil {
		return err
	}
	sd, err := windows.SecurityDescriptorFromString("D:P(A;OICI;FA;;;" + sid + ")")
	if err != nil {
		return err
	}
	dacl, _, err := sd.DACL()
	if err != nil {
		return err
	}
	return windows.SetNamedSecurityInfo(path, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION, nil, nil, dacl, nil)
}
func lock(path string) (func(), error) {
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return nil, err
	}
	handle, err := windows.CreateFile(name, windows.GENERIC_READ|windows.GENERIC_WRITE, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE, nil, windows.OPEN_ALWAYS, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		return nil, err
	}
	overlapped := new(windows.Overlapped)
	err = windows.LockFileEx(handle, windows.LOCKFILE_EXCLUSIVE_LOCK|windows.LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, overlapped)
	if err != nil {
		_ = windows.CloseHandle(handle)
		if errors.Is(err, windows.ERROR_LOCK_VIOLATION) {
			return nil, ErrRunning
		}
		return nil, err
	}
	return func() { _ = windows.UnlockFileEx(handle, 0, 1, 0, overlapped); _ = windows.CloseHandle(handle) }, nil
}
func endpoint(root string, role Role) string {
	sum := sha256.Sum256([]byte(strings.ToLower(filepath.Clean(root))))
	return `\\.\pipe\unic-desktop-` + hex.EncodeToString(sum[:16]) + "-" + string(role)
}
func listen(root string, role Role) (net.Listener, error) {
	sid, err := userSID()
	if err != nil {
		return nil, err
	}
	return winio.ListenPipe(endpoint(root, role), &winio.PipeConfig{SecurityDescriptor: "D:P(A;;GA;;;" + sid + ")", InputBufferSize: 4096, OutputBufferSize: 4096})
}
func dial(ctx context.Context, root string, role Role) (net.Conn, error) {
	return winio.DialPipeContext(ctx, endpoint(root, role))
}

// UI roles own their first ShowWindow call. STARTF_USESHOWWINDOW/SW_HIDE
// would override that call, leaving a cold quick-panel window invisible.
func configureRoleProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: syscall.CREATE_NEW_PROCESS_GROUP | windows.CREATE_NO_WINDOW}
}

func statFileStamp(path string) (fileStamp, bool) {
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return fileStamp{}, false
	}
	handle, err := windows.CreateFile(name, 0, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		return fileStamp{}, false
	}
	defer func() { _ = windows.CloseHandle(handle) }()
	return handleFileStamp(handle)
}

func fdFileStamp(f *os.File) (fileStamp, bool) {
	return handleFileStamp(windows.Handle(f.Fd()))
}

func handleFileStamp(handle windows.Handle) (fileStamp, bool) {
	var info windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &info); err != nil {
		return fileStamp{}, false
	}
	return fileStamp{
		dev:       uint64(info.VolumeSerialNumber),
		ino:       (uint64(info.FileIndexHigh) << 32) | uint64(info.FileIndexLow),
		size:      int64((uint64(info.FileSizeHigh) << 32) | uint64(info.FileSizeLow)),
		mtimeSec:  int64(info.LastWriteTime.HighDateTime),
		mtimeNsec: int64(info.LastWriteTime.LowDateTime),
	}, true
}
