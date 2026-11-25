package desktopprocess

import (
	"os"
	"path/filepath"
	"testing"
)

func TestFileStampTracksContentReplacement(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "unic-desktop.bin")
	if err := os.WriteFile(path, []byte("build-1-payload"), 0600); err != nil {
		t.Fatal(err)
	}

	hash1, stamp1, ok, err := hashFileWithStamp(path)
	if err != nil {
		t.Fatalf("hashFileWithStamp: %v", err)
	}
	if !ok {
		t.Fatal("expected valid file stamp for regular file")
	}

	// Unchanged file must keep identical stamp (cache hit).
	cur, curOK := statFileStamp(path)
	if !curOK || cur != stamp1 {
		t.Fatalf("unchanged file stamp drifted: got (%+v, %v), want (%+v, true)", cur, curOK, stamp1)
	}

	// Atomic rename replacement must change file identity (inode/FileIndex).
	next := filepath.Join(dir, "unic-desktop.next")
	if err := os.WriteFile(next, []byte("build-2-payload"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(next, path); err != nil {
		t.Fatal(err)
	}
	afterRename, ok := statFileStamp(path)
	if !ok {
		t.Fatal("statFileStamp after rename failed")
	}
	if afterRename == stamp1 {
		t.Fatalf("file stamp did not change after rename replacement: %+v", afterRename)
	}
	hash2, stamp2, ok, err := hashFileWithStamp(path)
	if err != nil || !ok {
		t.Fatalf("hashFileWithStamp after rename: err=%v ok=%v", err, ok)
	}
	if hash2 == hash1 {
		t.Fatal("expected different SHA-256 after replacement")
	}

	// In-place size modification must also invalidate the stamp.
	if err := os.WriteFile(path, []byte("build-3-longer-payload"), 0600); err != nil {
		t.Fatal(err)
	}
	afterWrite, ok := statFileStamp(path)
	if !ok {
		t.Fatal("statFileStamp after write failed")
	}
	if afterWrite == stamp2 {
		t.Fatalf("file stamp did not change after in-place write: %+v", afterWrite)
	}
}
