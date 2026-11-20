package desktopui

import (
	"bytes"
	"context"
	"encoding/binary"
	"hash/crc32"
	"image"
	"image/png"
	"io"
	"math/rand/v2"
	"net/http"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// The wrapper observes real service request ownership, including unread responses.
type previewResponse struct {
	io.ReadCloser
	ctx     context.Context
	release <-chan struct{}
	closed  func()
	once    sync.Once
	maxRead *atomic.Int64
}

func (b *previewResponse) Read(p []byte) (int, error) {
	if b.release != nil {
		select {
		case <-b.release:
		case <-b.ctx.Done():
			return 0, b.ctx.Err()
		}
	}
	if b.maxRead != nil {
		for n := b.maxRead.Load(); int64(len(p)) > n; n = b.maxRead.Load() {
			if b.maxRead.CompareAndSwap(n, int64(len(p))) {
				break
			}
		}
	}
	return b.ReadCloser.Read(p)
}
func (b *previewResponse) Close() error { b.once.Do(b.closed); return b.ReadCloser.Close() }

func imageResourceAcceptance(t *testing.T, ctx context.Context, socket string, items []api.HistoryItem, receipt map[string]any, restart func(bool)) {
	t.Helper()
	if len(items) < 8 {
		t.Fatal("missing image fixtures")
	}
	t.Run("request-admission", func(t *testing.T) {
		client := api.NewClient(socket)
		var mu sync.Mutex
		var opened []string
		active, peak := 0, 0
		release := make(chan struct{})
		first := make(chan struct{})
		client.WrapTransport(func(next http.RoundTripper) http.RoundTripper {
			return browsingTransport(func(r *http.Request) (*http.Response, error) {
				response, err := next.RoundTrip(r)
				if err != nil {
					return nil, err
				}
				if !strings.HasPrefix(response.Header.Get("Content-Type"), "image/") {
					return response, nil
				}
				mu.Lock()
				opened = append(opened, r.URL.Path)
				active++
				peak = max(peak, active)
				isFirst := len(opened) == 1
				mu.Unlock()
				var held <-chan struct{}
				if isFirst {
					held = release
					close(first)
				}
				response.Body = &previewResponse{ReadCloser: response.Body, ctx: r.Context(), release: held, closed: func() { mu.Lock(); active--; mu.Unlock() }}
				return response, nil
			})
		})
		done := make(chan entry, 6)
		start := func(c context.Context, it api.HistoryItem, edge int) {
			go func() { done <- loadSearchEntry(c, client, it, edge) }()
		}
		start(ctx, items[0], 256)
		select {
		case <-first:
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
		for _, it := range items[1:4] {
			start(ctx, it, 256)
		}
		cancelled, cancel := context.WithCancel(ctx)
		start(cancelled, items[4], 256)
		time.Sleep(30 * time.Millisecond)
		start(ctx, items[5], 0)
		time.Sleep(30 * time.Millisecond)
		begin := time.Now()
		cancel()
		select {
		case v := <-done:
			if v.contentError == "" {
				t.Error("cancelled queued preview succeeded")
			}
		case <-time.After(500 * time.Millisecond):
			t.Error("queued cancellation exceeded500ms")
		}
		cancelMS := time.Since(begin).Milliseconds()
		mu.Lock()
		queuedPeak := peak
		mu.Unlock()
		textOut, err := client.Submit(ctx, api.SubmitInput{RequestID: "image-admission-text", Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("text remains concurrent")}}}})
		if err != nil {
			t.Fatal(err)
		}
		page, err := client.SearchHistory(ctx, engine.HistorySearchRequest{Limit: 50})
		if err != nil {
			t.Fatal(err)
		}
		for _, hit := range page.Hits {
			if hit.Item.ID == string(textOut.HistoryID) {
				if text := loadSearchEntry(ctx, client, hit.Item, 0); text.text != "text remains concurrent" {
					t.Error("text blocked by image admission")
				}
			}
		}
		if queuedPeak != 1 {
			t.Errorf("queued image requests already own responses: peak=%d, want1", queuedPeak)
		}
		close(release)
		for range 5 {
			select {
			case <-done:
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
		}
		mu.Lock()
		defer mu.Unlock()
		if active != 0 {
			t.Errorf("open image responses remain: %d", active)
		}
		if len(opened) < 2 || !strings.Contains(opened[1], items[5].ID) {
			t.Errorf("selected request did not get next slot: %v", opened)
		}
		for _, path := range opened {
			if strings.Contains(path, items[4].ID) {
				t.Error("cancelled queued request opened a response")
			}
		}
		receipt["request_admission"] = map[string]any{"peak_open_responses": peak, "queued_cancel_ms": cancelMS, "requests_opened": len(opened), "remaining": active}
	})

	t.Run("stream-input", func(t *testing.T) {
		client := api.NewClient(socket)
		raster := image.NewNRGBA(image.Rect(0, 0, 1024, 1024))
		rng := rand.New(rand.NewPCG(1, 2))
		for i := 0; i < len(raster.Pix); i += 4 {
			v := rng.Uint32()
			raster.Pix[i] = byte(v)
			raster.Pix[i+1] = byte(v >> 8)
			raster.Pix[i+2] = byte(v >> 16)
			raster.Pix[i+3] = 255
		}
		var encoded bytes.Buffer
		if err := png.Encode(&encoded, raster); err != nil {
			t.Fatal(err)
		}
		valid := encoded.Bytes()
		out, err := client.Submit(ctx, api.SubmitInput{RequestID: "image-stream-resource", Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.PNG, Data: valid}}}})
		if err != nil {
			t.Fatal(err)
		}
		page, err := client.SearchHistory(ctx, engine.HistorySearchRequest{Limit: 50})
		if err != nil {
			t.Fatal(err)
		}
		var item api.HistoryItem
		for _, hit := range page.Hits {
			if hit.Item.ID == string(out.HistoryID) {
				item = hit.Item
			}
		}
		if item.ID == "" {
			t.Fatal("missing stream fixture")
		}
		var maxRead atomic.Int64
		var inject func() io.ReadCloser
		client.WrapTransport(func(next http.RoundTripper) http.RoundTripper {
			return browsingTransport(func(r *http.Request) (*http.Response, error) {
				response, err := next.RoundTrip(r)
				if err != nil {
					return nil, err
				}
				if strings.HasPrefix(response.Header.Get("Content-Type"), "image/") {
					if inject != nil {
						_ = response.Body.Close()
						response.Body = inject()
					}
					response.Body = &previewResponse{ReadCloser: response.Body, ctx: r.Context(), closed: func() {}, maxRead: &maxRead}
				}
				return response, nil
			})
		})
		v := loadSearchEntry(ctx, client, item, 256)
		if v.picture == nil {
			t.Fatal("valid streamed PNG failed", v.contentError)
		}
		if n := maxRead.Load(); n > 64<<10 {
			t.Errorf("thumbnail input buffered as a whole: largest read=%d, want<=65536", n)
		}
		receipt["stream_largest_read_bytes"] = maxRead.Load()
		// Repeated JPEG APP2 segments delay the dimension header past the existing byte limit.
		app := make([]byte, 65537)
		app[0] = 0xff
		app[1] = 0xe2
		app[2] = 0xff
		app[3] = 0xff
		oversizedHeader := append([]byte{0xff, 0xd8}, bytes.Repeat(app, 513)...)
		huge := append([]byte(nil), valid...)
		binary.BigEndian.PutUint32(huge[16:20], 16_000_001)
		binary.BigEndian.PutUint32(huge[29:33], crc32.ChecksumIEEE(huge[12:29]))
		for _, test := range []struct {
			name string
			body func() io.ReadCloser
		}{
			{"truncated", func() io.ReadCloser { return io.NopCloser(bytes.NewReader(valid[:len(valid)-16])) }},
			{"corrupt", func() io.ReadCloser { return io.NopCloser(strings.NewReader("not an image")) }},
			{"oversized-header", func() io.ReadCloser { return io.NopCloser(bytes.NewReader(oversizedHeader)) }},
			{"stream-error", func() io.ReadCloser {
				return io.NopCloser(io.MultiReader(bytes.NewReader(valid[:32]), errorPreviewReader{}))
			}},
			{"huge-raster", func() io.ReadCloser { return io.NopCloser(bytes.NewReader(huge)) }},
			{"trailing-over-limit", func() io.ReadCloser {
				return io.NopCloser(io.MultiReader(bytes.NewReader(valid), io.LimitReader(zeroPreviewReader{}, 32<<20)))
			}},
		} {
			t.Run(test.name, func(t *testing.T) {
				inject = test.body
				v := loadSearchEntry(ctx, client, item, 256)
				if v.picture != nil || v.contentError == "" {
					t.Fatal("invalid stream accepted")
				}
			})
		}
		inject = nil
		cancelled, cancel := context.WithCancel(ctx)
		cancel()
		begin := time.Now()
		v = loadSearchEntry(cancelled, client, item, 256)
		if v.picture != nil || v.contentError == "" {
			t.Error("cancelled stream accepted")
		}
		if time.Since(begin) > 500*time.Millisecond {
			t.Error("cancelled read exceeded500ms")
		}
		if v = loadSearchEntry(ctx, client, item, 256); v.picture == nil {
			t.Error("slot unavailable after malformed/cancelled inputs", v.contentError)
		}
	})
	t.Run("active-cancel-error-shutdown", func(t *testing.T) {
		client := api.NewClient(socket)
		opened := make(chan struct{}, 1)
		var hold atomic.Bool
		hold.Store(true)
		client.WrapTransport(func(next http.RoundTripper) http.RoundTripper {
			return browsingTransport(func(r *http.Request) (*http.Response, error) {
				response, err := next.RoundTrip(r)
				if err == nil && strings.HasPrefix(response.Header.Get("Content-Type"), "image/") && hold.Load() {
					response.Body = &previewResponse{ReadCloser: response.Body, ctx: r.Context(), release: make(chan struct{}), closed: func() {}}
					opened <- struct{}{}
				}
				return response, err
			})
		})
		activeCtx, cancel := context.WithCancel(ctx)
		done := make(chan entry, 1)
		go func() { done <- loadSearchEntry(activeCtx, client, items[0], 256) }()
		select {
		case <-opened:
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
		begin := time.Now()
		cancel()
		select {
		case v := <-done:
			if v.picture != nil {
				t.Error("active cancelled response published pixels")
			}
		case <-time.After(500 * time.Millisecond):
			t.Fatal("active cancellation exceeded500ms")
		}
		receipt["active_cancel_ms"] = time.Since(begin).Milliseconds()
		hold.Store(false)
		bad := items[0]
		bad.ID = "00000000000000000000000000000000"
		if v := loadSearchEntry(ctx, client, bad, 256); v.picture != nil || v.contentError == "" {
			t.Error("missing history unexpectedly previewed")
		}
		if v := loadSearchEntry(ctx, client, items[0], 256); v.picture == nil {
			t.Error("slot leaked after HTTP failure")
		}
		restart(false)
		short, stop := context.WithTimeout(ctx, time.Second)
		v := loadSearchEntry(short, client, items[0], 256)
		stop()
		if v.picture != nil || v.contentError == "" {
			t.Error("stopped service unexpectedly previewed")
		}
		restart(true)
		if v := loadSearchEntry(ctx, client, items[0], 256); v.picture == nil {
			t.Error("slot unavailable after service restart")
		}
	})

}

type zeroPreviewReader struct{}

func (zeroPreviewReader) Read(p []byte) (int, error) { clear(p); return len(p), nil }

type errorPreviewReader struct{}

func (errorPreviewReader) Read([]byte) (int, error) { return 0, io.ErrUnexpectedEOF }
