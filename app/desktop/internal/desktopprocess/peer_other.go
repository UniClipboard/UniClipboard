//go:build !windows && !darwin && !linux

package desktopprocess

import (
	"errors"

	"golang.org/x/sys/unix"
)

func processExecutable(int) (string, error) {
	return "", errors.New("desktop IPC peer executable verification unsupported")
}

func unixFileStamp(*unix.Stat_t) (fileStamp, bool) { return fileStamp{}, false }
