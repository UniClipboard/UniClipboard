package engine

import (
	"context"
	"io"

	"github.com/uniclipboard/unic/internal/directory"
	"github.com/uniclipboard/unic/sourceapp"
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
	SourceApplication *SourceApplication `json:"source_application,omitempty"`
	ID                ContentID          `json:"id"`
	Parts             []PartDescriptor   `json:"parts"`
	TotalSize         int64              `json:"total_size"`
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

// PersistentFileSource is implemented by a FileSource whose file can be found
// again after a restart. History then stores only the opaque reference (the
// desktop uses the absolute path) with the size and Version seen at commit,
// and never copies the file. The engine gives the reference back to
// Options.ResolveFileRef; it is a secret like a path and never leaves the
// database, a DTO, an event or a log.
type PersistentFileSource interface {
	FileSource
	PersistentRef() string
}

// ReceivedFile supplies a host with one complete received file, not a path.
// Reader is valid only until Options.StoreReceivedFile returns. The host must
// consume exactly Size bytes before returning a readable PersistentFileSource.
type ReceivedFile struct {
	Name   string
	MIME   string
	Size   int64
	Reader io.Reader
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

// SourceApplication is portable application provenance supplied by the host.
type SourceApplication = sourceapp.Application

// ContentInput is one clipboard item: alternative representations and files.
type ContentInput struct {
	SourceApplication *SourceApplication
	ObservationKey    string // local copy-action marker; never transmitted
	Representations   []RepresentationInput
	Files             []FileInput
}

// DirectorySource supplies a selected directory without exposing host paths.
// ReadDirectory returns an operation-local listing and verification callback.
// Open returns identity metadata and an empty stream.
type DirectorySource interface {
	PersistentFileSource
	ReadDirectory(context.Context) (DirectoryListing, error)
}

type DirectoryEntry struct {
	Path      string
	Directory bool
	Size      int64
	Source    FileSource
}

// ReceivedDirectory is a verified complete tree offered to a custom host store.
// OpenFile readers are valid only during StoreReceivedDirectory.
type ReceivedDirectory struct {
	Manifest DirectoryManifest
	OpenFile func(int) (io.ReadCloser, error)
}
type DirectoryManifest = directory.Manifest
type DirectoryRoot = directory.Root
type DirectoryManifestEntry = directory.Entry

// StoredDirectory contains private root references interpreted only by the host.
type StoredDirectory struct {
	Files []PersistentFileSource
	Roots []string
}

// DirectoryListing belongs to one preparation and is safe from other sends.
type DirectoryListing struct {
	Entries []DirectoryEntry
	Verify  func(context.Context) error
}
