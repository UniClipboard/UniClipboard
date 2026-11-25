package desktopui

import (
	"bytes"
	"context"
	"crypto/sha256"
	encodingbinary "encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/jpeg"
	"image/png"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// Count overlapping image-body consumption in the real service transport.
type imagePreviewBody struct {
	io.ReadCloser
	active, peak  *atomic.Int64
	start, finish sync.Once
}

func (b *imagePreviewBody) done() { b.finish.Do(func() { b.active.Add(-1) }) }
func (b *imagePreviewBody) Read(p []byte) (int, error) {
	b.start.Do(func() {
		n := b.active.Add(1)
		for old := b.peak.Load(); n > old; old = b.peak.Load() {
			if b.peak.CompareAndSwap(old, n) {
				break
			}
		}
		time.Sleep(40 * time.Millisecond)
	})
	n, err := b.ReadCloser.Read(p)
	if err != nil {
		b.done()
	}
	return n, err
}
func (b *imagePreviewBody) Close() error {
	b.start.Do(func() { b.active.Add(1) })
	b.done()
	return b.ReadCloser.Close()
}

func scenarioImagePreviews(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	private, err := os.MkdirTemp("/tmp", "unic-wake-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := os.RemoveAll(private); err != nil {
			t.Error(err)
		}
	}()
	exe := filepath.Join(private, "unicd")
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build: %v\n%s", err, out)
	}
	binary, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(binary)
	receipt := map[string]any{"passed": false, "unicd_sha256": hex.EncodeToString(sum[:]), "native_ui": false, "renderer": true}
	checks := []string{}
	defer func() {
		receipt["passed"] = !t.Failed()
		receipt["checks"] = checks
		data, err := json.MarshalIndent(receipt, "", "  ")
		if err != nil {
			t.Error(err)
			return
		}
		if err := os.WriteFile(os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"), data, 0600); err != nil {
			t.Error(err)
		}
	}()
	home := filepath.Join(private, "home")
	if err := os.MkdirAll(home, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600); err != nil {
		t.Fatal(err)
	}
	proc := exec.CommandContext(ctx, exe)
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "UNIC_") {
			proc.Env = append(proc.Env, value)
		}
	}
	proc.Env = append(proc.Env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	if err := proc.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = proc.Process.Kill(); _ = proc.Wait() }()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	wait := func(label string, ready func() bool) {
		t.Helper()
		deadline := time.Now().Add(15 * time.Second)
		for time.Now().Before(deadline) {
			if ready() {
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		t.Fatalf("timeout: %s", label)
	}
	wait("service", func() bool { _, err := client.Status(ctx); return err == nil })

	picture := image.NewRGBA(image.Rect(0, 0, 1024, 1024))
	originals := map[string][]byte{}
	for n := 0; n < 8; n++ {
		picture.SetRGBA(0, 0, color.RGBA{R: uint8(n + 1), A: 255})
		var b bytes.Buffer
		if err := png.Encode(&b, picture); err != nil {
			t.Fatal(err)
		}
		data := append([]byte(nil), b.Bytes()...)
		out, err := client.Submit(ctx, api.SubmitInput{RequestID: fmt.Sprintf("image-memory-%d", n), Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.PNG, Data: data}}}})
		if err != nil {
			t.Fatal(err)
		}
		originals[string(out.HistoryID)] = data
	}
	var active, peak atomic.Int64
	client.WrapTransport(func(next http.RoundTripper) http.RoundTripper {
		return browsingTransport(func(r *http.Request) (*http.Response, error) {
			response, err := next.RoundTrip(r)
			if err == nil && strings.HasPrefix(response.Header.Get("Content-Type"), "image/") {
				response.Body = &imagePreviewBody{ReadCloser: response.Body, active: &active, peak: &peak}
			}
			return response, err
		})
	})
	desktop := NewDesktop(ctx, client)
	defer desktop.Close()
	view := ui.NewTester(desktop.View, 1100, 720)
	wait("image metadata", func() bool {
		view.Advance(20 * time.Millisecond)
		return !desktop.d.live.loading && len(desktop.d.entries) == 8
	})
	d := desktop.d
	l := d.live
	l.prefetchDetails(d, d.entries, 0, 7)
	wait("image prefetch", func() bool {
		view.Advance(20 * time.Millisecond)
		return len(l.details) == 8 && len(l.detailReads) == 0
	})
	var pixels int64
	for _, v := range d.entries {
		w, h := ui.ImageSize(v.image)
		if v.id != d.selected && (w > 256 || h > 256) {
			t.Fatalf("neighbor retains full raster %.0fx%.0f", w, h)
		}
		if v.id != d.selected {
			pixels += int64(w * h * 4)
		}
	}
	receipt["thumbnail_rgba_bytes"] = pixels
	if pixels > 32<<20 {
		t.Fatal("thumbnail budget exceeded")
	}
	checks = append(checks, "eight real PNG entries prefetch bounded thumbnails")
	old := d.selected
	selected := d.entries[7].id
	d.selectEntry(selected)
	view.Frame()
	wait("original selection", func() bool {
		view.Advance(20 * time.Millisecond)
		v := d.current()
		if v == nil {
			return false
		}
		w, h := ui.ImageSize(v.image)
		return w == 1024 && h == 1024 && len(l.detailReads) == 0
	})
	wait("former original downgraded", func() bool {
		view.Advance(20 * time.Millisecond)
		v := l.details[old]
		w, h := ui.ImageSize(v.image)
		return w > 0 && w <= 256 && h <= 256 && len(l.detailReads) == 0
	})
	stream, err := client.HistoryContent(ctx, selected, 0)
	if err != nil {
		t.Fatal(err)
	}
	stored, err := io.ReadAll(stream)
	_ = stream.Close()
	if err != nil || !bytes.Equal(stored, originals[selected]) {
		t.Fatal("stored original changed", err)
	}
	checks = append(checks, "selection upgrades to original dimensions; previous selection returns to thumbnail; original bytes unchanged")
	quick := newLiveQuick(ctx, client, "system", nil)
	defer quick.live.d.live.cancel()
	panel := ui.NewTester(quick.panel, 380, 366)
	wait("quick panel images", func() bool {
		panel.Advance(20 * time.Millisecond)
		return !quick.live.d.live.loading && len(quick.rows) == 8
	})
	for i, row := range quick.rows {
		if row.id == selected {
			quick.selected = i
		}
	}
	quick.hovered = -1
	quick.keyboard = true
	quick.previewID = selected
	quick.pendingPreview = selected
	quick.previewDue = time.Time{}
	panel.Frame()
	wait("quick original", func() bool {
		panel.Advance(20 * time.Millisecond)
		v := quick.live.d.current()
		if v == nil {
			return false
		}
		w, h := ui.ImageSize(v.image)
		return v.id == selected && w == 1024 && h == 1024
	})
	checks = append(checks, "quick-panel selected image retains original resolution")
	// A portrait EXIF JPEG file must keep its orientation through thumbnail and full reads.
	var jpegData bytes.Buffer
	if err := jpeg.Encode(&jpegData, image.NewRGBA(image.Rect(0, 0, 600, 300)), nil); err != nil {
		t.Fatal(err)
	}
	exif := []byte{'E', 'x', 'i', 'f', 0, 0, 'I', 'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0}
	var segment bytes.Buffer
	segment.Write([]byte{0xff, 0xe1})
	_ = encodingbinary.Write(&segment, encodingbinary.BigEndian, uint16(len(exif)+2))
	segment.Write(exif)
	oriented := append(append(append([]byte{}, jpegData.Bytes()[:2]...), segment.Bytes()...), jpegData.Bytes()[2:]...)
	path := filepath.Join(private, "oriented.jpg")
	if err := os.WriteFile(path, oriented, 0600); err != nil {
		t.Fatal(err)
	}
	out, err := client.Submit(ctx, api.SubmitInput{RequestID: "image-memory-exif", Snapshot: clip.Snapshot{Files: []clip.File{{Name: "oriented.jpg", Path: path, Size: int64(len(oriented))}}}})
	if err != nil {
		t.Fatal(err)
	}
	id := string(out.HistoryID)
	wait("EXIF file metadata", func() bool { view.Advance(20 * time.Millisecond); _, ok := l.items[id]; return ok && !l.loading })
	l.prefetchDetails(d, d.entries, 0, 8)
	wait("EXIF thumbnail", func() bool {
		view.Advance(20 * time.Millisecond)
		v, ok := l.details[id]
		return ok && v.image != nil && len(l.detailReads) == 0
	})
	w, h := ui.ImageSize(l.details[id].image)
	if w >= h || h > 256 {
		t.Fatalf("thumbnail lost EXIF orientation: %.0fx%.0f", w, h)
	}
	d.selectEntry(id)
	view.Frame()
	wait("EXIF original", func() bool {
		view.Advance(20 * time.Millisecond)
		v := d.current()
		if v == nil {
			return false
		}
		w, h := ui.ImageSize(v.image)
		return w == 300 && h == 600 && len(l.detailReads) == 0
	})
	checks = append(checks, "JPEG EXIF orientation and original dimensions preserved")
	preview := d.current().picture
	if preview == nil {
		t.Fatal("selected image has no decoded preview")
	}
	view.Advance(20 * time.Millisecond)
	view.Frame()
	if d.current().picture != preview || preview.released {
		t.Fatal("software repaint did not preserve the selected preview and its CPU pixels")
	}
	checks = append(checks, "software repaint preserves the selected preview and its CPU pixels")
	receipt["cached_details"] = len(l.details)
	receipt["peak_image_body_readers"] = peak.Load()
	if peak.Load() != 1 {
		t.Fatalf("image reads overlap: peak=%d", peak.Load())
	}
	checks = append(checks, "image bodies consumed serially across both live windows")
	quick.live.d.live.cancel()
	desktop.Close()
	wait("original preview readers closed", func() bool { return active.Load() == 0 })
	items := make([]api.HistoryItem, 0, 8)
	for _, it := range l.items {
		if it.Kind == "image" {
			items = append(items, it)
		}
	}
	// A selected original still needs its exact encoded reconstruction bytes.
	// Failures: geometric input-buffer copies, trusting an unbounded size hint,
	// truncation/extra bytes, changed originals, or measuring concurrent views.
	entropy := image.NewRGBA(image.Rect(0, 0, 1024, 1024))
	seed := uint32(0x58231)
	for i := 0; i < len(entropy.Pix); i += 4 {
		seed ^= seed << 13
		seed ^= seed >> 17
		seed ^= seed << 5
		entropy.Pix[i], entropy.Pix[i+1], entropy.Pix[i+2], entropy.Pix[i+3] = byte(seed), byte(seed>>8), byte(seed>>16), 255
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, entropy); err != nil {
		t.Fatal(err)
	}
	created, err := client.Submit(ctx, api.SubmitInput{RequestID: "original-input-allocation", Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.PNG, Data: encoded.Bytes()}}}})
	if err != nil {
		t.Fatal(err)
	}
	item, err := client.GetHistory(ctx, string(created.HistoryID))
	if err != nil {
		t.Fatal(err)
	}
	var originalReadSize, originalReadCalls atomic.Int64
	client.WrapTransport(func(next http.RoundTripper) http.RoundTripper {
		return browsingTransport(func(r *http.Request) (*http.Response, error) {
			response, err := next.RoundTrip(r)
			if err == nil && strings.HasPrefix(response.Header.Get("Content-Type"), "image/") {
				response.Body = &previewResponse{ReadCloser: response.Body, ctx: ctx, closed: func() {}, maxRead: &originalReadSize, readCalls: &originalReadCalls}
			}
			return response, err
		})
	})
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	original := loadSearchEntry(ctx, client, item, 0)
	runtime.ReadMemStats(&after)
	allocated := after.TotalAlloc - before.TotalAlloc
	budget := uint64(2*len(entropy.Pix) + 2*encoded.Len() + (1 << 20))
	receipt["original_input_allocation"] = map[string]any{"encoded_bytes": encoded.Len(), "raster_bytes": len(entropy.Pix), "allocated_bytes": allocated, "budget_bytes": budget, "forced_gc": false, "largest_read_bytes": originalReadSize.Load(), "read_calls": originalReadCalls.Load()}
	if original.picture == nil || original.picture.width != 1024 || original.picture.height != 1024 {
		t.Fatal("selected high-entropy original failed")
	}
	if allocated > budget {
		t.Fatalf("selected original allocates excess input copies: %d > %d", allocated, budget)
	}
	if originalReadSize.Load() > 64<<10 {
		t.Fatalf("selected original issues oversized reads: %d > 65536", originalReadSize.Load())
	}
	checks = append(checks, "real-service high-entropy selected original stays within the input-copy allocation budget without forced GC")
	for _, hint := range []int64{-1, 0, 1, int64(encoded.Len() + 4096), (32 << 20) + 1, 1 << 62} {
		altered := api.NewClient(filepath.Join(home, "unic.sock"))
		altered.WrapTransport(func(next http.RoundTripper) http.RoundTripper {
			return browsingTransport(func(r *http.Request) (*http.Response, error) {
				response, err := next.RoundTrip(r)
				if err == nil && strings.HasPrefix(response.Header.Get("Content-Type"), "image/") {
					response.ContentLength = hint
				}
				return response, err
			})
		})
		v := loadSearchEntry(ctx, altered, item, 0)
		if v.picture == nil || v.picture.width != 1024 || v.picture.height != 1024 {
			t.Fatalf("inaccurate original length hint %d changed preview", hint)
		}
	}
	checks = append(checks, "real-service originals accept unknown, low, high and over-limit length hints as hints without changing decoding or resource bounds")

	imageResourceAcceptance(t, ctx, filepath.Join(home, "unic.sock"), items, receipt, func(start bool) {
		t.Helper()
		if !start {
			_ = proc.Process.Kill()
			_ = proc.Wait()
			return
		}
		replacement := exec.CommandContext(ctx, exe)
		replacement.Env = proc.Env
		proc = replacement
		if err := proc.Start(); err != nil {
			t.Fatal(err)
		}
		wait("restarted image service", func() bool { _, err := client.Status(ctx); return err == nil })
	})
}
