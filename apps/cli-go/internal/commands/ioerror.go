package commands

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"syscall"
	"unicode"
	"unicode/utf8"
)

// rustIOError renders an OS-level error like Rust's `std::io::Error` Display
// (`Permission denied (os error 13)`), which is what the Rust CLI prints.
func rustIOError(err error) string {
	var errno syscall.Errno
	if !errors.As(err, &errno) {
		return err.Error()
	}
	text := errno.Error()
	if r, size := utf8.DecodeRuneInString(text); size > 0 {
		text = string(unicode.ToUpper(r)) + text[size:]
	}
	return fmt.Sprintf("%s (os error %d)", text, int(errno))
}

// ensureOutputDir mirrors the Rust `resolve_out_dir` helpers shared by `get`
// and `recv`: create the directory when missing, reject non-directories, and
// return the canonical absolute path.
func ensureOutputDir(dir string) (string, error) {
	info, err := os.Stat(dir)
	if err != nil {
		if mkErr := os.MkdirAll(dir, 0o777); mkErr != nil {
			return "", errors.New("Failed to create output directory: " + rustIOError(mkErr))
		}
	} else if !info.IsDir() {
		return "", errors.New("Output path is not a directory: " + dir)
	}
	canonical, err := canonicalize(dir)
	if err != nil {
		return "", errors.New("Failed to canonicalize output directory: " + rustIOError(err))
	}
	return canonical, nil
}

// canonicalize resolves path like Rust's `Path::canonicalize` (realpath).
func canonicalize(path string) (string, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	return filepath.EvalSymlinks(abs)
}
