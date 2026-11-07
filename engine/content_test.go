package engine

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func pngBytes(t *testing.T) []byte {
	t.Helper()
	var b bytes.Buffer
	if err := png.Encode(&b, image.NewGray(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	return b.Bytes()
}

func rep(mime, s string) RepresentationInput {
	return RepresentationInput{MIME: mime, Size: int64(len(s)), Reader: strings.NewReader(s)}
}

func fileIn(name, s string) FileInput {
	return FileInput{Name: name, Size: int64(len(s)), Reader: strings.NewReader(s)}
}

// pathSource is how a desktop host keeps a file where it is.
type pathSource struct{ path string }

func (p pathSource) Open(context.Context) (io.ReadCloser, FileMeta, error) {
	f, err := os.Open(p.path)
	if err != nil {
		return nil, FileMeta{}, err
	}
	st, err := f.Stat()
	if err != nil {
		f.Close()
		return nil, FileMeta{}, err
	}
	return f, FileMeta{Size: st.Size(), Version: fmt.Sprint(st.ModTime().UnixNano())}, nil
}

func newContentNode(t *testing.T, mutate func(*Options)) node {
	t.Helper()
	dir := t.TempDir()
	opts, f := newOpts(t, dir)
	if mutate != nil {
		mutate(&opts)
	}
	e, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { e.Close(bg) })
	return node{e, f, dir}
}

func readAll(t *testing.T, e *Engine, l LeaseID, p PartID) string {
	t.Helper()
	r, err := e.ReadContent(bg, l, p)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	b, err := io.ReadAll(r)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func stored(e *Engine) int64 { e.cs.mu.Lock(); defer e.cs.mu.Unlock(); return e.cs.stored }

func contentDirs(n node) []string {
	d, _ := filepath.Glob(filepath.Join(n.dir, "inbox", contentDirPrefix+"*"))
	return d
}

func TestImportAndRead(t *testing.T) {
	n := newContentNode(t, nil)
	img := pngBytes(t)
	h, err := n.e.ImportContent(bg, ContentInput{
		Representations: []RepresentationInput{rep("Text/Plain", "héllo"), rep(MimeHTML, "<b>x</b>"),
			{MIME: MimePNG, Size: int64(len(img)), Reader: bytes.NewReader(img)}},
		Files: []FileInput{fileIn("../../etc/passwd", "secret file")},
	})
	if err != nil {
		t.Fatal(err)
	}
	d := h.Content
	if len(d.Parts) != 4 || d.TotalSize != int64(len("héllo")+8+len(img)+11) {
		t.Fatalf("descriptor = %+v", d)
	}
	if readAll(t, n.e, h.LeaseID, d.Parts[0].ID) != "héllo" || readAll(t, n.e, h.LeaseID, d.Parts[3].ID) != "secret file" {
		t.Fatal("wrong bytes")
	}
	if d.Parts[3].Name != "passwd" || d.Parts[3].Kind != PartFile || d.Parts[3].Referenced {
		t.Fatalf("file part = %+v", d.Parts[3])
	}
	if js, _ := json.Marshal(h); strings.Contains(string(js), n.dir) {
		t.Fatalf("a path leaked into the handle: %s", js)
	}
	// The copy stays inside the engine's own directory.
	ds := contentDirs(n)
	if len(ds) != 1 {
		t.Fatalf("content dirs = %v", ds)
	}
	p, err := n.e.LocalFilePath(bg, h.LeaseID, d.Parts[3].ID)
	if err != nil || !strings.HasPrefix(p, ds[0]+string(filepath.Separator)) || filepath.Base(p) != "passwd" {
		t.Fatalf("LocalFilePath = %q, %v", p, err)
	}
	if _, err := n.e.LocalFilePath(bg, h.LeaseID, d.Parts[0].ID); code(err) != CodeUnsupported {
		t.Fatalf("path of a representation: %v", err)
	}
	if stored(n.e) != d.TotalSize {
		t.Fatalf("stored = %d, want %d", stored(n.e), d.TotalSize)
	}
}

func TestImportValidation(t *testing.T) {
	n := newContentNode(t, nil)
	long := func(size int64, s string) RepresentationInput {
		return RepresentationInput{MIME: MimeText, Size: size, Reader: strings.NewReader(s)}
	}
	cases := map[string]struct {
		in   ContentInput
		want Code
	}{
		"empty":           {ContentInput{}, CodeInvalidArgument},
		"duplicate mime":  {ContentInput{Representations: []RepresentationInput{rep(MimeText, "a"), rep(MimeText, "b")}}, CodeInvalidArgument},
		"zero size":       {ContentInput{Representations: []RepresentationInput{rep(MimeText, "")}}, CodeInvalidArgument},
		"unknown format":  {ContentInput{Representations: []RepresentationInput{rep("x/unknown", "a")}}, CodeUnsupported},
		"bad utf8":        {ContentInput{Representations: []RepresentationInput{rep(MimeText, "\xff\xfe")}}, CodeInvalidArgument},
		"bad png":         {ContentInput{Representations: []RepresentationInput{rep(MimePNG, "not a png")}}, CodeInvalidArgument},
		"short stream":    {ContentInput{Representations: []RepresentationInput{long(10, "abc")}}, CodeInvalidArgument},
		"extra bytes":     {ContentInput{Representations: []RepresentationInput{long(2, "abcdef")}}, CodeInvalidArgument},
		"short file":      {ContentInput{Files: []FileInput{{Name: "a", Size: 9, Reader: strings.NewReader("abc")}}}, CodeInvalidArgument},
		"extra file":      {ContentInput{Files: []FileInput{{Name: "a", Size: 1, Reader: strings.NewReader("abc")}}}, CodeInvalidArgument},
		"no name":         {ContentInput{Files: []FileInput{fileIn(" ", "x")}}, CodeInvalidArgument},
		"no source":       {ContentInput{Files: []FileInput{{Name: "a", Size: 1}}}, CodeInvalidArgument},
		"both sources":    {ContentInput{Files: []FileInput{{Name: "a", Size: 1, Reader: strings.NewReader("a"), Source: pathSource{"x"}}}}, CodeInvalidArgument},
		"negative size":   {ContentInput{Files: []FileInput{{Name: "a", Size: -1, Reader: strings.NewReader("")}}}, CodeInvalidArgument},
		"over file limit": {ContentInput{Files: []FileInput{{Name: "a", Size: 3 << 30, Reader: strings.NewReader("")}}}, CodeLimitExceeded},
		"reader fails":    {ContentInput{Files: []FileInput{{Name: "a", Size: 5, Reader: io.MultiReader(strings.NewReader("ab"), errReader{})}}}, CodeInvalidArgument},
	}
	for name, c := range cases {
		if _, err := n.e.ImportContent(bg, c.in); code(err) != c.want {
			t.Errorf("%s: %v, want %s", name, err, c.want)
		}
	}
	// Nothing is left behind by any failure.
	if stored(n.e) != 0 || len(contentDirs(n)) != 0 || len(n.e.cs.contents) != 0 {
		t.Fatalf("leftovers: stored=%d dirs=%v contents=%d", stored(n.e), contentDirs(n), len(n.e.cs.contents))
	}
}

type errReader struct{}

func (errReader) Read([]byte) (int, error) { return 0, errors.New("disk on fire at /secret/path") }

func TestUserLimitsApplyToImport(t *testing.T) {
	n := newContentNode(t, func(o *Options) {
		o.Settings = DefaultSettings()
		o.Settings.Limits = Limits{MaxFileBytes: 4, MaxTotalBytes: 6}
	})
	if _, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("a", "12345")}}); code(err) != CodeLimitExceeded {
		t.Fatalf("file over limit: %v", err)
	}
	if _, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("a", "1234"), fileIn("b", "123")}}); code(err) != CodeLimitExceeded {
		t.Fatalf("total over limit: %v", err)
	}
	if h, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("a", "1234")}}); err != nil {
		t.Fatal(err)
	} else {
		n.e.ReleaseContent(bg, h.LeaseID)
	}
}

func TestQuotaIsReservedAndRolledBack(t *testing.T) {
	n := newContentNode(t, func(o *Options) {
		o.Settings = DefaultSettings()
		o.Settings.Limits.MaxTotalBytes = 100
		o.Resources.MaxStoredBytes = 100
	})
	h1, err := n.e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{rep(MimeText, strings.Repeat("a", 60))}})
	if err != nil {
		t.Fatal(err)
	}
	two := ContentInput{Representations: []RepresentationInput{rep(MimeText, strings.Repeat("b", 60))}}
	if _, err := n.e.ImportContent(bg, two); code(err) != CodeResourceExhausted {
		t.Fatalf("over quota: %v", err)
	}
	// A failed import gives its reservation back.
	if _, err := n.e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{{MIME: MimeText, Size: 30, Reader: strings.NewReader("short")}}}); err == nil {
		t.Fatal("short import succeeded")
	}
	if stored(n.e) != 60 {
		t.Fatalf("stored = %d, want 60", stored(n.e))
	}
	if _, err := n.e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{rep(MimeText, strings.Repeat("c", 40))}}); err != nil {
		t.Fatalf("the quota was not given back: %v", err)
	}
	// Releasing the first content frees its share.
	n.e.ReleaseContent(bg, h1.LeaseID)
	if stored(n.e) != 40 {
		t.Fatalf("stored after release = %d", stored(n.e))
	}
	if _, err := n.e.ImportContent(bg, two); err != nil {
		t.Fatalf("after release: %v", err)
	}
}

func TestLeasesAndReferences(t *testing.T) {
	n := newContentNode(t, nil)
	h, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("f.txt", "data")}})
	if err != nil {
		t.Fatal(err)
	}
	part := h.Content.Parts[0].ID
	dir := contentDirs(n)[0]

	// A second lease keeps the content when the first goes.
	h2, err := n.e.RetainContent(bg, h.Content.ID)
	if err != nil || h2.LeaseID == h.LeaseID {
		t.Fatalf("retain = %+v, %v", h2, err)
	}
	for i := 0; i < 3; i++ { // releasing again changes nothing for the other holder
		if err := n.e.ReleaseContent(bg, h.LeaseID); err != nil {
			t.Fatalf("release #%d: %v", i, err)
		}
	}
	if readAll(t, n.e, h2.LeaseID, part) != "data" {
		t.Fatal("content lost while another lease holds it")
	}
	if _, err := n.e.ReadContent(bg, h.LeaseID, part); code(err) != CodeResourceExpired {
		t.Fatalf("read with a released lease: %v", err)
	}

	// An open reader outlives the last lease, and its Close deletes the file.
	r, err := n.e.ReadContent(bg, h2.LeaseID, part)
	if err != nil {
		t.Fatal(err)
	}
	n.e.ReleaseContent(bg, h2.LeaseID)
	if _, err := os.Stat(dir); err != nil {
		t.Fatal("the file went while a reader was open")
	}
	if b, _ := io.ReadAll(r); string(b) != "data" {
		t.Fatalf("read %q", b)
	}
	r.Close()
	r.Close()
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatalf("content dir survived the last reference: %v", err)
	}
	if stored(n.e) != 0 {
		t.Fatalf("stored = %d", stored(n.e))
	}
	if _, err := n.e.RetainContent(bg, h.Content.ID); code(err) != CodeResourceExpired {
		t.Fatalf("retain of freed content: %v", err)
	}
	if _, err := n.e.RetainContent(bg, ContentID(n.e.instance)+"-nope"); code(err) != CodeNotFound {
		t.Fatalf("retain of unknown content: %v", err)
	}
	if err := n.e.ReleaseContent(bg, LeaseID(n.e.instance)+"-nope"); code(err) != CodeNotFound {
		t.Fatalf("release of unknown lease: %v", err)
	}
	if err := n.e.ReleaseContent(bg, "0000000000000000-abc"); code(err) != CodeResourceExpired {
		t.Fatalf("release of another instance's lease: %v", err)
	}
	if _, err := n.e.GetOperation(bg, "0000000000000000-abc"); code(err) != CodeResourceExpired {
		t.Fatalf("operation of another instance: %v", err)
	}
}

func TestReferencedFilesAreNotCopied(t *testing.T) {
	n := newContentNode(t, func(o *Options) {
		o.Settings = DefaultSettings()
		o.Settings.Limits.MaxTotalBytes = 1500
		o.Resources.MaxStoredBytes = 1500 // two copies of the file would not fit: only copies count
	})
	path := filepath.Join(t.TempDir(), "big.bin")
	data := strings.Repeat("x", 1000)
	os.WriteFile(path, []byte(data), 0o600)

	h, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{{Name: "big.bin", Size: 1000, Source: pathSource{path}}}})
	if err != nil {
		t.Fatal(err)
	}
	h2, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{{Name: "big.bin", Size: 1000, Source: pathSource{path}}}})
	if err != nil {
		t.Fatalf("a second reference needs no storage: %v", err)
	}
	n.e.ReleaseContent(bg, h2.LeaseID)
	if !h.Content.Parts[0].Referenced || stored(n.e) != 0 || len(contentDirs(n)) != 0 {
		t.Fatalf("a reference was copied: %+v stored=%d dirs=%v", h.Content.Parts[0], stored(n.e), contentDirs(n))
	}
	if readAll(t, n.e, h.LeaseID, h.Content.Parts[0].ID) != data {
		t.Fatal("wrong bytes")
	}
	if _, err := n.e.LocalFilePath(bg, h.LeaseID, h.Content.Parts[0].ID); code(err) != CodeUnsupported {
		t.Fatalf("path of a referenced file: %v", err)
	}
	if js, _ := json.Marshal(h); strings.Contains(string(js), path) {
		t.Fatal("the host's path leaked")
	}

	// Edited, truncated, replaced or deleted: reading fails, never returns other bytes.
	later := time.Now().Add(2 * time.Second)
	os.WriteFile(path, []byte(data), 0o600)
	os.Chtimes(path, later, later)
	if _, err := n.e.ReadContent(bg, h.LeaseID, h.Content.Parts[0].ID); code(err) != CodeSourceChanged {
		t.Fatalf("touched file: %v", err)
	}
	os.WriteFile(path, []byte("short"), 0o600)
	if _, err := n.e.ReadContent(bg, h.LeaseID, h.Content.Parts[0].ID); code(err) != CodeSourceChanged {
		t.Fatalf("truncated file: %v", err)
	}
	os.Remove(path)
	if _, err := n.e.ReadContent(bg, h.LeaseID, h.Content.Parts[0].ID); code(err) != CodeSourceChanged {
		t.Fatalf("deleted file: %v", err)
	}
	// A failed read does not leak a reference.
	n.e.ReleaseContent(bg, h.LeaseID)
	if len(n.e.cs.contents) != 0 {
		t.Fatal("content survived its last release")
	}

	// A missing file or a wrong declared size is refused at import.
	if _, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{{Name: "x", Size: 1, Source: pathSource{path}}}}); code(err) != CodeSourceChanged {
		t.Fatalf("missing file: %v", err)
	}
	os.WriteFile(path, []byte("12345"), 0o600)
	if _, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{{Name: "x", Size: 4, Source: pathSource{path}}}}); code(err) != CodeInvalidArgument {
		t.Fatalf("wrong declared size: %v", err)
	}
}

func TestImportStopsWhenContextEnds(t *testing.T) {
	n := newContentNode(t, nil)
	pr, pw := io.Pipe()
	defer pw.Close()
	ctx, cancel := context.WithCancel(bg)
	res := make(chan error, 1)
	go func() {
		_, err := n.e.ImportContent(ctx, ContentInput{Files: []FileInput{{Name: "slow", Size: 1 << 20, Reader: pr}}})
		res <- err
	}()
	pw.Write([]byte("partial"))
	time.Sleep(50 * time.Millisecond)
	cancel()
	pw.Write([]byte("more")) // unblocks the read that was waiting
	select {
	case err := <-res:
		if code(err) != CodeCancelled || !errors.Is(err, context.Canceled) {
			t.Fatalf("import = %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("import ignored cancellation")
	}
	if stored(n.e) != 0 || len(contentDirs(n)) != 0 {
		t.Fatalf("leftovers: stored=%d dirs=%v", stored(n.e), contentDirs(n))
	}
}

func TestCloseAbortsReadsAndCleansUp(t *testing.T) {
	n := newContentNode(t, nil)
	h, _ := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("a", strings.Repeat("z", 100))}})
	r, err := n.e.ReadContent(bg, h.LeaseID, h.Content.Parts[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	if err := n.e.Close(bg); err != nil {
		t.Fatal(err)
	}
	if _, err := r.Read(make([]byte, 10)); code(err) != CodeClosed {
		t.Fatalf("read after Close: %v", err)
	}
	r.Close()
	if len(contentDirs(n)) != 0 {
		t.Fatal("Close left content behind")
	}
	if _, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("a", "x")}}); code(err) != CodeClosed {
		t.Fatalf("import after Close: %v", err)
	}
}

func TestNewRemovesOnlyItsOwnLeftovers(t *testing.T) {
	dir := t.TempDir()
	inbox := filepath.Join(dir, "inbox")
	for _, d := range []string{contentDirPrefix + "dead", "holiday-photos", "clip-1234"} {
		os.MkdirAll(filepath.Join(inbox, d), 0o700)
	}
	opts, _ := newOpts(t, dir)
	e, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close(bg)
	if _, err := os.Stat(filepath.Join(inbox, contentDirPrefix+"dead")); !os.IsNotExist(err) {
		t.Fatal("a dead instance's content was kept")
	}
	for _, keep := range []string{"holiday-photos", "clip-1234"} {
		if _, err := os.Stat(filepath.Join(inbox, keep)); err != nil {
			t.Fatalf("%s must not be touched: %v", keep, err)
		}
	}
}

func TestContentWorksWhileStopped(t *testing.T) {
	n := newContentNode(t, nil)
	h, err := n.e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{rep(MimeText, "kept")}})
	if err != nil {
		t.Fatal(err)
	}
	start(t, n)
	n.e.Stop(bg)
	if readAll(t, n.e, h.LeaseID, h.Content.Parts[0].ID) != "kept" {
		t.Fatal("Stop must not release leases")
	}
	if err := n.e.ReleaseContent(bg, h.LeaseID); err != nil {
		t.Fatal(err)
	}
}
