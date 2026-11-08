package v1migration

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/hex"
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/zeebo/blake3"
)

func TestExternalFileIntegrityAndDirectoryArchive(t *testing.T) {
	content := []byte("original bytes")
	sum := blake3.Sum256(content)
	path := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(path, content, 0o600); err != nil {
		t.Fatal(err)
	}
	p := Part{Path: path, Size: int64(len(content)), Hash: "blake3v1:" + hex.EncodeToString(sum[:])}
	unknownSize, err := externalFilePart(path, 0, false, p.Hash)
	if err != nil || unknownSize.Size != p.Size {
		t.Fatal("legacy NULL size rejected", err)
	}
	if _, err := externalFilePart(path, 0, true, p.Hash); err == nil {
		t.Fatal("known size mismatch accepted")
	}
	archive, err := archiveDirectory([]archiveMember{
		{name: "folder/", directory: true},
		{name: "folder/nested/file", part: p},
		{name: "folder/empty/", directory: true},
	})
	if err != nil {
		t.Fatal(err)
	}
	z, err := zip.NewReader(bytes.NewReader(archive), int64(len(archive)))
	if err != nil || len(z.File) != 3 {
		t.Fatal("directory layout lost", err)
	}
	r, err := z.File[1].Open()
	if err != nil {
		t.Fatal(err)
	}
	got, err := io.ReadAll(r)
	_ = r.Close()
	if err != nil || !bytes.Equal(got, content) {
		t.Fatal("archive content changed")
	}
	if err := os.WriteFile(path, bytes.Repeat([]byte("x"), len(content)), 0o600); err != nil {
		t.Fatal(err)
	}
	r, err = p.Open()
	if err != nil {
		t.Fatal(err)
	}
	_, err = io.Copy(io.Discard, r)
	_ = r.Close()
	if err == nil {
		t.Fatal("modified source file accepted")
	}
}

func TestMigrationRejectsOverlappingDirectories(t *testing.T) {
	root, _, f := sourceFixture(t)
	s, err := Open(context.Background(), root, "fixture", []byte(f.Passphrase), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	for _, target := range []string{root, filepath.Join(root, "new-target")} {
		if _, err := s.Run(context.Background(), RunOptions{Target: target}); err == nil {
			t.Fatal("overlapping directories accepted")
		}
	}
	link := filepath.Join(t.TempDir(), "link")
	if err := os.Symlink(root, link); err != nil {
		t.Skip("symlink unavailable")
	}
	if _, err := s.Run(context.Background(), RunOptions{Target: filepath.Join(link, "new-target")}); err == nil {
		t.Fatal("overlap through symlink accepted")
	}
}
