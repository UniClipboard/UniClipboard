package mobile

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"

	"github.com/uniclipboard/unic/engine"
)

// FileStream streams bytes from a host-owned file (e.g., iOS security-scoped URL or Android SAF URI).
// Returning an empty byte slice with nil error signals io.EOF.
type FileStream interface {
	ReadChunk(maxBytes int) ([]byte, error)
	Close() error
}

// FileOpener opens a host-owned file for reading without exposing paths or URIs to Engine DTOs.
type FileOpener interface {
	Open() (FileStream, error)
	Size() int64
	Version() string
}

type bridgedStreamReader struct {
	stream FileStream
	buf    []byte
	eof    bool
}

func (r *bridgedStreamReader) Read(p []byte) (int, error) {
	if len(p) == 0 {
		return 0, nil
	}
	if len(r.buf) > 0 {
		n := copy(p, r.buf)
		r.buf = r.buf[n:]
		return n, nil
	}
	if r.eof {
		return 0, io.EOF
	}
	chunk, err := r.stream.ReadChunk(len(p))
	if len(chunk) > 0 {
		n := copy(p, chunk)
		if n < len(chunk) {
			r.buf = append(r.buf[:0], chunk[n:]...)
		}
		if err != nil {
			// Retain error on next read unless EOF.
			if errors.Is(err, io.EOF) {
				r.eof = true
				return n, nil
			}
			return n, err
		}
		return n, nil
	}
	if err != nil {
		return 0, err
	}
	r.eof = true
	return 0, io.EOF
}

func (r *bridgedStreamReader) Close() error {
	if r.stream == nil {
		return nil
	}
	return r.stream.Close()
}

type bridgedFileSource struct {
	opener FileOpener
}

func (s *bridgedFileSource) Open(ctx context.Context) (io.ReadCloser, engine.FileMeta, error) {
	if err := ctx.Err(); err != nil {
		return nil, engine.FileMeta{}, err
	}
	stream, err := s.opener.Open()
	if err != nil {
		return nil, engine.FileMeta{}, err
	}
	meta := engine.FileMeta{
		Size:    s.opener.Size(),
		Version: s.opener.Version(),
	}
	return &bridgedStreamReader{stream: stream}, meta, nil
}

// ContentStream reads an Engine leased part in bounded chunks across the gomobile boundary.
type ContentStream struct {
	reader *engine.ContentReader
}

// ReadChunk reads up to maxBytes bytes. Returns nil, nil at EOF.
func (s *ContentStream) ReadChunk(maxBytes int) ([]byte, error) {
	if s == nil || s.reader == nil {
		return nil, wrapErr(errors.New("stream closed"))
	}
	if maxBytes <= 0 {
		maxBytes = 64 << 10
	}
	if maxBytes > 4<<20 {
		maxBytes = 4 << 20
	}
	buf := make([]byte, maxBytes)
	n, err := s.reader.Read(buf)
	if n > 0 {
		out := buf[:n]
		if err != nil && !errors.Is(err, io.EOF) {
			return out, wrapErr(err)
		}
		return out, nil
	}
	if errors.Is(err, io.EOF) {
		return nil, nil
	}
	return nil, wrapErr(err)
}

// WriteToFile streams the content into destPath atomically via a temp file in the same directory, then closes the stream.
func (s *ContentStream) WriteToFile(destPath string) (int64, error) {
	if s == nil || s.reader == nil {
		return 0, wrapErr(errors.New("stream closed"))
	}
	defer func() { _ = s.Close() }()
	dir := filepath.Dir(destPath)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return 0, wrapErr(err)
	}
	tmp, err := os.CreateTemp(dir, ".unic-part-*")
	if err != nil {
		return 0, wrapErr(err)
	}
	tmpName := tmp.Name()
	cleanup := true
	defer func() {
		if cleanup {
			_ = os.Remove(tmpName)
		}
	}()
	buf := make([]byte, 64<<10)
	n, copyErr := io.CopyBuffer(tmp, s.reader, buf)
	closeErr := tmp.Close()
	if copyErr != nil {
		return 0, wrapErr(copyErr)
	}
	if closeErr != nil {
		return 0, wrapErr(closeErr)
	}
	if err := os.Rename(tmpName, destPath); err != nil {
		return 0, wrapErr(err)
	}
	cleanup = false
	return n, nil
}

// Close closes the underlying ContentReader idempotently.
func (s *ContentStream) Close() error {
	if s == nil || s.reader == nil {
		return nil
	}
	err := s.reader.Close()
	s.reader = nil
	return wrapErr(err)
}
