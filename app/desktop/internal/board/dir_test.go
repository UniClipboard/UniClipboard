package board

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"github.com/uniclipboard/unic/internal/clip"
)

func TestDirRoundTrip(t *testing.T) {
	d := Dir(filepath.Join(t.TempDir(), "board"))
	if s, err := d.Read(); err != nil || !s.Empty() {
		t.Fatalf("a missing directory is an empty clipboard: %+v %v", s, err)
	}
	want := clip.Snapshot{Parts: []clip.Part{{Format: "text/plain", Data: []byte("hi")}, {Format: "text/html", Data: []byte("<b>hi</b>")}, {Format: "image/png", Data: []byte{1, 2, 3}}}}
	if err := d.Write(want); err != nil {
		t.Fatal(err)
	}
	got, err := d.Read()
	if err != nil || got.Hash() != want.Hash() {
		t.Fatalf("read %+v %v", got, err)
	}
	// Writing replaces everything, including representations the new content lacks.
	if err := d.Write(clip.Snapshot{Parts: []clip.Part{{Format: "text/plain", Data: []byte("only")}}}); err != nil {
		t.Fatal(err)
	}
	got, _ = d.Read()
	if len(got.Parts) != 1 || !bytes.Equal(got.Parts[0].Data, []byte("only")) {
		t.Fatalf("stale representations survived: %+v", got)
	}
}

func TestDirFilesWinOverText(t *testing.T) {
	dir := t.TempDir()
	f := filepath.Join(dir, "a.bin")
	if err := os.WriteFile(f, []byte("12345"), 0o600); err != nil {
		t.Fatal(err)
	}
	d := Dir(filepath.Join(dir, "board"))
	if err := d.Write(clip.Snapshot{Files: []clip.File{{Name: "a.bin", Path: f}}}); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(string(d), "text"), []byte("a.bin"), 0o600); err != nil {
		t.Fatal(err)
	}
	s, err := d.Read()
	if err != nil || len(s.Files) != 1 || s.Files[0].Size != 5 || len(s.Parts) != 0 {
		t.Fatalf("files must win: %+v %v", s, err)
	}
}
