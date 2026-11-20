//go:build !windows && !darwin && !linux

package desktopprocess

import "errors"

func processExecutable(int) (string, error) {
	return "", errors.New("desktop IPC peer executable verification unsupported")
}
