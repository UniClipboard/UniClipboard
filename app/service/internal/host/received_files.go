package host

import (
	"context"
	"encoding/base64"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/uniclipboard/unic/engine"
)

const receivedFilePrefix = "desktop-received:"

// receivedPathSource belongs to the desktop host. Engine stores its reference
// unchanged and never interprets the encoded path or receives deletion rights.
type receivedPathSource struct{ pathSource }

func (s receivedPathSource) PersistentRef() string {
	return receivedFilePrefix + base64.RawURLEncoding.EncodeToString([]byte(s.Path))
}

// desktopFilePath interprets both local-copy references and custom receive references.
func desktopFilePath(ref string) (string, error) {
	if value, ok := strings.CutPrefix(ref, receivedFilePrefix); ok {
		decoded, err := base64.RawURLEncoding.DecodeString(value)
		if err != nil {
			return "", errors.New("invalid desktop file reference")
		}
		ref = string(decoded)
	}
	if !filepath.IsAbs(ref) {
		return "", errors.New("invalid desktop file reference")
	}
	return ref, nil
}
func resolveDesktopFile(ref string) engine.FileSource {
	path, err := desktopFilePath(ref)
	if err != nil {
		return nil
	}
	if strings.HasPrefix(ref, receivedFilePrefix) {
		return receivedPathSource{pathSource{Path: path}}
	}
	return pathSource{Path: path}
}

// receivedFileWriter captures the active startup destination. Configuration
// changes require restart. Only this host interprets the selected directory.
func receivedFileWriter(directory, dataDir string) (func(context.Context, engine.ReceivedFile) (engine.PersistentFileSource, error), error) {
	if directory == "" {
		return nil, nil
	}
	resolved, err := filepath.EvalSymlinks(directory)
	if err != nil {
		return nil, errors.New("custom receive directory is unavailable")
	}
	resolved, err = filepath.Abs(resolved)
	if err != nil {
		return nil, errors.New("invalid custom receive directory")
	}
	internal, err := filepath.Abs(dataDir)
	if err != nil {
		return nil, err
	}
	if resolved == internal || strings.HasPrefix(resolved, internal+string(os.PathSeparator)) || strings.HasPrefix(internal, resolved+string(os.PathSeparator)) {
		return nil, errors.New("custom receive directory must be outside application storage")
	}
	identity, err := os.Stat(resolved)
	if err != nil || !identity.IsDir() {
		return nil, errors.New("custom receive directory is unavailable")
	}
	return func(ctx context.Context, file engine.ReceivedFile) (engine.PersistentFileSource, error) {
		current, err := os.Stat(resolved)
		if err != nil || !os.SameFile(identity, current) {
			return nil, errors.New("custom receive directory changed")
		}
		root, err := os.OpenRoot(resolved)
		if err != nil {
			return nil, err
		}
		defer func() { _ = root.Close() }()
		// Treat the received name as display text. Exclusive creation prevents replacement.
		if file.Name == "" || filepath.Base(file.Name) != file.Name || strings.ContainsAny(file.Name, "/\\") || file.Name == ".." {
			return nil, errors.New("invalid received filename")
		}
		name := randomID() + "-" + file.Name
		out, err := root.OpenFile(name, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if err != nil {
			return nil, err
		}
		_, err = io.CopyN(out, contextFileReader{ctx, file.Reader}, file.Size)
		if err == nil {
			err = out.Sync()
		}
		if cerr := out.Close(); err == nil {
			err = cerr
		}
		// Even partial destinations remain user-managed; never remove them here.
		if err != nil {
			return nil, errors.New("custom received file could not be saved")
		}
		return receivedPathSource{pathSource{Path: filepath.Join(resolved, name)}}, nil
	}, nil
}

type contextFileReader struct {
	ctx    context.Context
	reader io.Reader
}

func (r contextFileReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.reader.Read(p)
}
