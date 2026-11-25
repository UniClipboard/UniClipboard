//go:build !windows

package desktopprocess

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"

	"golang.org/x/sys/unix"

	"github.com/uniclipboard/unic/internal/servicelaunch"
)

func samePath(a, b string) bool { return a == b }
func privateDir(path string) error {
	if err := os.Mkdir(path, 0700); err != nil && !errors.Is(err, os.ErrExist) {
		return err
	}
	stat, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !stat.IsDir() || stat.Mode()&os.ModeSymlink != 0 || stat.Mode().Perm()&0077 != 0 {
		return errors.New("desktop runtime directory must be private and not a symlink")
	}
	var native unix.Stat_t
	if err := unix.Lstat(path, &native); err != nil {
		return err
	}
	if native.Uid != uint32(os.Geteuid()) {
		return errors.New("desktop runtime directory belongs to another user")
	}
	return nil
}
func lock(path string) (func(), error) {
	fd, err := unix.Open(path, unix.O_CREAT|unix.O_RDWR|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0600)
	if err != nil {
		return nil, err
	}
	if err = unix.Flock(fd, unix.LOCK_EX|unix.LOCK_NB); err != nil {
		_ = unix.Close(fd)
		if errors.Is(err, unix.EWOULDBLOCK) {
			return nil, ErrRunning
		}
		return nil, err
	}
	return func() { _ = unix.Flock(fd, unix.LOCK_UN); _ = unix.Close(fd) }, nil
}
func endpoint(root string, role Role) string {
	// Keep AF_UNIX paths within the macOS 104-byte limit, including long profiles.
	sum := sha256.Sum256([]byte(root))
	return filepath.Join("/tmp", fmt.Sprintf("unic-desktop-%d", os.Getuid()), hex.EncodeToString(sum[:16])+"-"+string(role)+".sock")
}
func listen(root string, role Role) (net.Listener, error) {
	path := endpoint(root, role)
	if err := privateDir(filepath.Dir(path)); err != nil {
		return nil, err
	}
	if conn, err := net.Dial("unix", path); err == nil {
		_ = conn.Close()
		return nil, errors.New("unrecognized live desktop role listener")
	}
	if stat, err := os.Lstat(path); err == nil {
		if stat.Mode()&os.ModeSocket == 0 {
			return nil, errors.New("desktop role endpoint is not a socket")
		}
		if err := os.Remove(path); err != nil {
			return nil, err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	ln, err := net.Listen("unix", path)
	if err != nil {
		return nil, fmt.Errorf("listen desktop role: %w", err)
	}
	if err := os.Chmod(path, 0600); err != nil {
		_ = ln.Close()
		return nil, err
	}
	return ln, nil
}
func dial(ctx context.Context, root string, role Role) (net.Conn, error) {
	var d net.Dialer
	return d.DialContext(ctx, "unix", endpoint(root, role))
}

func configureRoleProcess(cmd *exec.Cmd) { servicelaunch.ConfigureProcess(cmd) }

func statFileStamp(path string) (fileStamp, bool) {
	var st unix.Stat_t
	if err := unix.Stat(path, &st); err != nil {
		return fileStamp{}, false
	}
	return unixFileStamp(&st)
}

func fdFileStamp(f *os.File) (fileStamp, bool) {
	var st unix.Stat_t
	if err := unix.Fstat(int(f.Fd()), &st); err != nil {
		return fileStamp{}, false
	}
	return unixFileStamp(&st)
}
