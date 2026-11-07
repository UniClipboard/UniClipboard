package engine

import (
	"context"
	"io"
)

// ContentID names stored content; LeaseID is one holder's claim on it; PartID
// names one part within a content. All are opaque.
type (
	ContentID string
	LeaseID   string
	PartID    string
)

// PartKind tells an in-memory representation from a file.
type PartKind string

const (
	PartRepresentation PartKind = "representation"
	PartFile           PartKind = "file"
)

// PartDescriptor describes one part. It carries no bytes and no path.
type PartDescriptor struct {
	ID   PartID   `json:"id"`
	Kind PartKind `json:"kind"`
	MIME string   `json:"mime"`
	Name string   `json:"name,omitempty"` // files: display name only
	Size int64    `json:"size"`
	// Referenced is true for a file the host keeps: the engine opens it again
	// when it needs the bytes and fails with source_changed if it changed.
	Referenced bool `json:"referenced,omitempty"`
}

// ContentDescriptor describes stored content.
type ContentDescriptor struct {
	ID        ContentID        `json:"id"`
	Parts     []PartDescriptor `json:"parts"`
	TotalSize int64            `json:"total_size"`
}

// ContentHandle is what an import returns: the content and the caller's lease
// on it. Release the lease when done.
type ContentHandle struct {
	Content ContentDescriptor `json:"content"`
	LeaseID LeaseID           `json:"lease_id"`
}

// FileMeta is what a FileSource reports about its file.
type FileMeta struct {
	Size int64
	// Version is an opaque change marker (a modification time, an etag). If it
	// differs from the value seen at import, the file is treated as changed.
	Version string
}

// FileSource lets the host keep a file where it is. The engine never learns a
// path or URI: it calls Open whenever it needs the bytes. Open may be called
// from any goroutine and more than once.
type FileSource interface {
	Open(ctx context.Context) (io.ReadCloser, FileMeta, error)
}

// RepresentationInput is an in-memory form of the content (text, HTML, PNG).
type RepresentationInput struct {
	MIME   string
	Size   int64 // exact number of bytes Reader yields; must be > 0
	Reader io.Reader
}

// FileInput is a file. Give exactly one of Reader (the bytes are copied into
// storage the engine owns) or Source (the file stays where it is).
type FileInput struct {
	Name   string // display name; never used as a path
	MIME   string // default application/octet-stream
	Size   int64  // exact size; may be 0
	Reader io.Reader
	Source FileSource
}

// ContentInput is one clipboard item: alternative representations of one
// thing, plus files.
type ContentInput struct {
	Representations []RepresentationInput
	Files           []FileInput
}
