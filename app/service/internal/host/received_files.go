package host

import (
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/localcontent"
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
	if localcontent.IsDirectoryRef(ref) {
		return localcontent.ResolveDirectory(ref)
	}
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

// receivedDirectoryWriter publishes a complete tree into one exclusive batch.
// The ordinary writer has already validated this destination outside DataDir.
func receivedDirectoryWriter(directory string, dataDir string) func(context.Context, engine.ReceivedDirectory) (engine.StoredDirectory, error) {
	if directory == "" {
		return nil
	}
	resolved, resolveErr := filepath.EvalSymlinks(directory)
	identity, statErr := os.Stat(resolved)
	return func(ctx context.Context, in engine.ReceivedDirectory) (engine.StoredDirectory, error) {
		var out engine.StoredDirectory
		dir := resolved
		current, err := os.Lstat(dir)
		if resolveErr != nil || statErr != nil || err != nil || !current.IsDir() || !os.SameFile(identity, current) {
			return out, errors.New("custom directory unavailable")
		}
		root, err := os.OpenRoot(dir)
		if err != nil {
			return out, err
		}
		defer root.Close()
		stage := ".unic-directory-stage-" + randomID()
		final := "unic-directory-" + randomID()
		journal, err := recordDirectoryStage(dataDir, dir, stage, identity)
		if err != nil {
			return out, err
		}
		if err := root.Mkdir(stage, 0700); err != nil {
			_ = os.Remove(journal)
			return out, err
		}
		ok := false
		defer func() {
			if ok || root.RemoveAll(stage) == nil {
				_ = os.Remove(journal)
			}
		}()
		files := 0
		for _, e := range in.Manifest.Entries {
			if !e.Directory {
				files++
			}
		}
		paths := make([]string, files)
		for i := range in.Manifest.Roots {
			sub := fmt.Sprintf("r%d", i)
			if err := root.Mkdir(filepath.Join(stage, sub), 0700); err != nil {
				return out, err
			}
		}
		for _, entry := range in.Manifest.Entries {
			if err := ctx.Err(); err != nil {
				return out, err
			}
			relative := filepath.Join(fmt.Sprintf("r%d", entry.Root), in.Manifest.Roots[entry.Root].Name, filepath.FromSlash(entry.Path))
			target := filepath.Join(stage, relative)
			if entry.Directory {
				if err := root.Mkdir(target, 0700); err != nil {
					return out, err
				}
				continue
			}
			r, err := in.OpenFile(entry.File)
			if err != nil {
				return out, err
			}
			w, err := root.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
			if err != nil {
				r.Close()
				return out, err
			}
			n, err := io.Copy(w, contextFileReader{ctx: ctx, reader: r})
			_ = r.Close()
			if err == nil {
				err = w.Sync()
			}
			closeErr := w.Close()
			if err != nil {
				return out, err
			}
			if closeErr != nil {
				return out, closeErr
			}
			if n != entry.Size {
				return out, errors.New("directory member changed")
			}
			paths[entry.File] = relative
		}
		if err := ctx.Err(); err != nil {
			return out, err
		}
		if err := root.Rename(stage, final); err != nil {
			return out, err
		}
		ok = true
		for _, p := range paths {
			out.Files = append(out.Files, receivedPathSource{pathSource{Path: filepath.Join(dir, final, p)}})
		}
		for i, r := range in.Manifest.Roots {
			path := filepath.Join(dir, final, fmt.Sprintf("r%d", i), r.Name)
			if r.Directory {
				st, err := os.Stat(path)
				if err != nil {
					return out, err
				}
				out.Roots = append(out.Roots, localcontent.NewDirectorySource(path, st).PersistentRef())
			} else {
				out.Roots = append(out.Roots, receivedPathSource{pathSource{Path: path}}.PersistentRef())
			}
		}
		return out, nil
	}
}
