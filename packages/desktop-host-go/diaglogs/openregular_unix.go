//go:build !windows

package diaglogs

import (
	"errors"
	"os"
	"syscall"
)

// openRegular opens path read-only without following a link, so an entry swapped for a symlink after the directory
// listing is refused instead of exported. O_NONBLOCK keeps a swapped-in FIFO from blocking the open.
func openRegular(path string) (*os.File, error) {
	file, err := os.OpenFile(path, os.O_RDONLY|syscall.O_NOFOLLOW|syscall.O_NONBLOCK, 0)
	if err != nil {
		return nil, err
	}
	info, err := file.Stat()
	if err != nil {
		file.Close()
		return nil, err
	}
	if !info.Mode().IsRegular() {
		file.Close()
		return nil, errors.New("not a regular file")
	}
	return file, nil
}
