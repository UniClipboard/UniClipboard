//go:build darwin

package nativeboard

import (
	"bytes"
	"crypto/rand"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"unsafe"

	"github.com/ebitengine/purego/objc"

	"github.com/uniclipboard/unic/internal/localcontent"
)

func newTestBoard(t *testing.T) *Board {
	t.Helper()
	b, err := New()
	if err != nil {
		t.Skipf("macOS pasteboard unavailable: %v", err)
	}
	b.name = "org.uniclipboard.test." + rand.Text()
	t.Cleanup(func() {
		_ = b.use(func(board objc.ID) error {
			send(board, "releaseGlobally")
			return nil
		})
	})
	return b
}

func fileReferenceURLString(t *testing.T, path string) string {
	t.Helper()
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := send(send(objc.ID(objc.GetClass("NSAutoreleasePool")), "alloc"), "init")
	defer send(pool, "drain")

	u := send(objc.ID(objc.GetClass("NSURL")), "fileURLWithPath:", nsString(path))
	ref := send(u, "fileReferenceURL")
	if ref == 0 {
		t.Fatalf("fileReferenceURL returned nil for %q", path)
	}
	s := goString(send(ref, "absoluteString"))
	if !strings.HasPrefix(s, "file:///.file/id=") {
		t.Fatalf("unexpected file reference URL %q", s)
	}
	return s
}

func TestReadFinderFileReferenceURLs(t *testing.T) {
	b := newTestBoard(t)
	dir := t.TempDir()
	path1 := filepath.Join(dir, "测试 文件 #1 %20?.txt")
	path2 := filepath.Join(dir, "second file.bin")
	if err := os.WriteFile(path1, []byte("hello"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path2, []byte("world!"), 0o600); err != nil {
		t.Fatal(err)
	}
	want1, err := filepath.EvalSymlinks(path1)
	if err != nil {
		t.Fatal(err)
	}
	want2, err := filepath.EvalSymlinks(path2)
	if err != nil {
		t.Fatal(err)
	}

	ref1 := fileReferenceURLString(t, path1)
	ref2 := fileReferenceURLString(t, path2)

	// Simulate Finder writing multiple NSPasteboardItems with file:///.file/id=...
	// in public.file-url (as UTF-8 data, matching Finder/AppKit behavior).
	if err := b.use(func(board objc.ID) error {
		items := send(objc.ID(objc.GetClass("NSMutableArray")), "array")
		for _, ref := range []string{ref1, ref2} {
			item := send(send(objc.ID(objc.GetClass("NSPasteboardItem")), "alloc"), "init")
			defer send(item, "release")
			raw := []byte(ref)
			data := send(objc.ID(objc.GetClass("NSData")), "dataWithBytes:length:", unsafe.Pointer(unsafe.SliceData(raw)), uint64(len(raw)))
			runtime.KeepAlive(raw)
			send(item, "setData:forType:", data, nsString("public.file-url"))
			send(items, "addObject:", item)
		}
		send(board, "clearContents")
		if !objc.Send[bool](board, objc.RegisterName("writeObjects:"), items) {
			t.Fatal("writeObjects failed")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	snap, err := b.Read()
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(snap.Files) != 2 {
		t.Fatalf("got %d files (%+v), want 2", len(snap.Files), snap.Files)
	}
	if snap.Files[0].Path != want1 || snap.Files[0].Size != 5 || snap.Files[0].Name != filepath.Base(want1) {
		t.Fatalf("file[0] = %+v, want path=%q size=5", snap.Files[0], want1)
	}
	if snap.Files[1].Path != want2 || snap.Files[1].Size != 6 || snap.Files[1].Name != filepath.Base(want2) {
		t.Fatalf("file[1] = %+v, want path=%q size=6", snap.Files[1], want2)
	}
	in, err := localcontent.Input(snap)
	if err != nil {
		t.Fatalf("localcontent.Input: %v", err)
	}
	if len(in.Files) != 2 {
		t.Fatalf("localcontent.Input files = %d, want 2", len(in.Files))
	}
}

func TestReadFileImageWithFileReferenceURL(t *testing.T) {
	b := newTestBoard(t)
	path := filepath.Join(t.TempDir(), "photo.png")
	var buf bytes.Buffer
	if err := png.Encode(&buf, image.NewNRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, buf.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}
	ref := fileReferenceURLString(t, path)

	if err := b.use(func(board objc.ID) error {
		item := send(send(objc.ID(objc.GetClass("NSPasteboardItem")), "alloc"), "init")
		defer send(item, "release")
		send(item, "setString:forType:", nsString(ref), nsString("public.file-url"))
		rawPNG := buf.Bytes()
		data := send(objc.ID(objc.GetClass("NSData")), "dataWithBytes:length:", unsafe.Pointer(unsafe.SliceData(rawPNG)), uint64(len(rawPNG)))
		runtime.KeepAlive(rawPNG)
		send(item, "setData:forType:", data, nsString("public.png"))
		items := send(objc.ID(objc.GetClass("NSMutableArray")), "array")
		send(items, "addObject:", item)
		send(board, "clearContents")
		send(board, "writeObjects:", items)
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	snap, err := b.Read()
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(snap.Files) != 1 || filepath.Base(snap.Files[0].Path) != "photo.png" {
		t.Fatalf("got files %+v, want photo.png", snap.Files)
	}
	gotPNG := b.ReadFileImage(snap.CopyID)
	if len(gotPNG) == 0 {
		t.Fatal("ReadFileImage returned empty PNG for file reference URL")
	}
}

func TestReadUnresolvableFileReference(t *testing.T) {
	b := newTestBoard(t)
	if err := b.use(func(board objc.ID) error {
		send(board, "clearContents")
		item := send(send(objc.ID(objc.GetClass("NSPasteboardItem")), "alloc"), "init")
		defer send(item, "release")
		send(item, "setString:forType:", nsString("file:///.file/id=0.0"), nsString("public.file-url"))
		items := send(objc.ID(objc.GetClass("NSMutableArray")), "array")
		send(items, "addObject:", item)
		send(board, "writeObjects:", items)
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	snap, err := b.Read()
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(snap.Files) != 0 {
		t.Fatalf("unresolvable file reference leaked files: %+v", snap.Files)
	}

	path := filepath.Join(t.TempDir(), "fallback.txt")
	if err := os.WriteFile(path, []byte("ok"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := b.use(func(board objc.ID) error {
		send(board, "clearContents")
		item := send(send(objc.ID(objc.GetClass("NSPasteboardItem")), "alloc"), "init")
		defer send(item, "release")
		send(item, "setString:forType:", nsString("file:///.file/id=0.0"), nsString("public.file-url"))
		items := send(objc.ID(objc.GetClass("NSMutableArray")), "array")
		send(items, "addObject:", item)
		send(board, "writeObjects:", items)

		types := send(objc.ID(objc.GetClass("NSMutableArray")), "array")
		send(types, "addObject:", nsString("NSFilenamesPboardType"))
		send(board, "addTypes:owner:", types, objc.ID(0))
		legacy := send(objc.ID(objc.GetClass("NSMutableArray")), "array")
		send(legacy, "addObject:", nsString(path))
		send(board, "setPropertyList:forType:", legacy, nsString("NSFilenamesPboardType"))
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	snap, err = b.Read()
	if err != nil {
		t.Fatalf("Read fallback: %v", err)
	}
	if len(snap.Files) != 1 || snap.Files[0].Path != path {
		t.Fatalf("got files %+v, want fallback path %q", snap.Files, path)
	}
}

func TestReadLongPercentEncodedFileURL(t *testing.T) {
	b := newTestBoard(t)
	// Build a path whose UTF-8 byte length is < 1024 (valid macOS path)
	// while its percent-encoded file:/// URL exceeds 1024 bytes.
	base := t.TempDir()
	seg := strings.Repeat("中", 40) // 120 UTF-8 bytes, 360 percent-encoded bytes
	dir := filepath.Join(base, seg, seg, seg)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "测试.txt")
	if err := os.WriteFile(path, []byte("long"), 0o600); err != nil {
		t.Fatal(err)
	}

	if err := b.use(func(board objc.ID) error {
		u := send(objc.ID(objc.GetClass("NSURL")), "fileURLWithPath:", nsString(path))
		rawURL := send(u, "absoluteString")
		if n := objc.Send[uint64](rawURL, objc.RegisterName("lengthOfBytesUsingEncoding:"), uint64(4)); n <= 1024 {
			t.Fatalf("expected percent-encoded URL > 1024 bytes, got %d", n)
		}
		item := send(send(objc.ID(objc.GetClass("NSPasteboardItem")), "alloc"), "init")
		defer send(item, "release")
		send(item, "setString:forType:", rawURL, nsString("public.file-url"))
		items := send(objc.ID(objc.GetClass("NSMutableArray")), "array")
		send(items, "addObject:", item)
		send(board, "clearContents")
		if !objc.Send[bool](board, objc.RegisterName("writeObjects:"), items) {
			t.Fatal("writeObjects failed")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	snap, err := b.Read()
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(snap.Files) != 1 || snap.Files[0].Path != path || snap.Files[0].Size != 4 {
		t.Fatalf("got files %+v, want path=%q size=4", snap.Files, path)
	}
}
