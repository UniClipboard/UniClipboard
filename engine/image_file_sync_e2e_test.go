package engine

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/jpeg"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"testing"
)

func TestExternalImageFileProcessE2E(t *testing.T) {
	n := newContentNode(t, func(o *Options) {
		instance := chanInstanceID("example.image-files")
		instance.ImageFilesAsImages = true
		o.Plugins = []PluginInstance{instance}
	})
	start(t, n)
	if wanted, err := n.e.WantsClipboardFileImage(bg); err != nil || wanted {
		t.Fatalf("capture requested without an eligible channel: %v %v", wanted, err)
	}
	addCh(t, n, "example.image-files")
	if wanted, err := n.e.WantsClipboardFileImage(bg); err != nil || !wanted {
		t.Fatalf("declared capability did not request capture: %v %v", wanted, err)
	}
	img := image.NewRGBA(image.Rect(0, 0, 8, 4))
	img.Set(0, 0, color.Black)
	var jpg bytes.Buffer
	if err := jpeg.Encode(&jpg, img, nil); err != nil {
		t.Fatal(err)
	}
	file := func(name string, data []byte) FileInput {
		return FileInput{Name: name, Size: int64(len(data)), Reader: bytes.NewReader(data)}
	}
	commitInput := func(id string, input ContentInput) SubmissionResult {
		t.Helper()
		h, err := n.e.ImportContent(bg, input)
		if err != nil {
			t.Fatal(err)
		}
		defer n.e.ReleaseContent(bg, h.LeaseID)
		result, err := n.e.CommitContent(bg, CommitRequest{RequestID: RequestID(id), LeaseID: h.LeaseID})
		if err != nil {
			t.Fatal(err)
		}
		return result
	}
	commit := func(id string, files ...FileInput) SubmissionResult {
		t.Helper()
		return commitInput(id, ContentInput{Files: files})
	}
	updateSettings(t, n.e, func(s *Settings) {
		p := DefaultBackendPolicy()
		p.Files = false
		s.BackendPolicies["example.image-files"] = p
	})
	result := commit("single-jpeg-file", file("screenshot.jpg", jpg.Bytes()))
	if result.SyncDisposition != SyncQueued {
		t.Fatalf("single JPEG with only image sync enabled: %+v", result)
	}
	page, err := n.e.ListHistory(bg, ListHistoryRequest{Filter: HistoryFilter{Kinds: []HistoryKind{HistoryFiles}}})
	if err != nil || len(page.Items) != 1 || page.Items[0].ID != result.HistoryID || len(page.Items[0].Parts) != 1 || page.Items[0].Parts[0].Kind != PartFile {
		t.Fatalf("expected original file in history: %+v, %v", page, err)
	}
	received := waitReceived(t, n.e, 1)[0]
	h, err := n.e.RetainContent(bg, received.Content.ID)
	if err != nil {
		t.Fatal(err)
	}
	defer n.e.ReleaseContent(bg, h.LeaseID)
	if len(h.Content.Parts) != 1 || h.Content.Parts[0].MIME != MimePNG || h.Content.Parts[0].Kind != PartRepresentation {
		t.Fatalf("expected one PNG representation: %+v", h.Content)
	}
	body, err := n.e.ReadContent(bg, h.LeaseID, h.Content.Parts[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	payload, err := io.ReadAll(body)
	_ = body.Close()
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := png.Decode(bytes.NewReader(payload))
	if err != nil || decoded.Bounds() != img.Bounds() {
		t.Fatalf("invalid projected PNG: %v", err)
	}
	var preferred bytes.Buffer
	if err := png.Encode(&preferred, image.NewRGBA(image.Rect(0, 0, 3, 5))); err != nil {
		t.Fatal(err)
	}
	alternative := commitInput("clipboard-image-and-file", ContentInput{
		Files: []FileInput{file("underlying.jpg", jpg.Bytes())}, FileImagePNG: preferred.Bytes(),
	})
	if alternative.SyncDisposition != SyncQueued {
		t.Fatalf("clipboard image alternative was not queued: %+v", alternative)
	}
	var echo ReceivedContent
	eventually(t, "clipboard image alternative echoed", func() bool {
		receivedItems, err := n.e.ListReceived(bg)
		if err != nil {
			return false
		}
		for _, item := range receivedItems {
			if item.ReceiptID != received.ReceiptID {
				echo = item
				return true
			}
		}
		return false
	})
	alternativeLease, err := n.e.RetainContent(bg, echo.Content.ID)
	if err != nil {
		t.Fatal(err)
	}
	defer n.e.ReleaseContent(bg, alternativeLease.LeaseID)
	body, err = n.e.ReadContent(bg, alternativeLease.LeaseID, alternativeLease.Content.Parts[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	gotPNG, err := io.ReadAll(body)
	_ = body.Close()
	if err != nil || !bytes.Equal(gotPNG, preferred.Bytes()) {
		t.Fatal("the clipboard PNG was not preferred over the backing file")
	}
	page, err = n.e.ListHistory(bg, ListHistoryRequest{Filter: HistoryFilter{Kinds: []HistoryKind{HistoryFiles}}})
	if err != nil || len(page.Items) != 2 || page.Items[0].ID != alternative.HistoryID || len(page.Items[0].Parts) != 1 || page.Items[0].Parts[0].Kind != PartFile {
		t.Fatalf("the alternative was persisted with the file: %+v, %v", page, err)
	}
	for _, tc := range []struct {
		id    string
		files []FileInput
	}{
		{"not-an-image", []FileInput{file("fake.png", []byte("no pixels"))}},
		{"multi-file", []FileInput{file("one.jpg", jpg.Bytes()), file("two.jpg", jpg.Bytes())}},
	} {
		if got := commit(tc.id, tc.files...); got.SyncDisposition != SyncPolicyFilter {
			t.Fatalf("%s unexpectedly submitted: %+v", tc.id, got)
		}
	}
	updateSettings(t, n.e, func(s *Settings) {
		p := DefaultBackendPolicy()
		p.Images = false
		s.BackendPolicies["example.image-files"] = p
	})
	if wanted, err := n.e.WantsClipboardFileImage(bg); err != nil || wanted {
		t.Fatalf("capture requested while image policy was off: %v %v", wanted, err)
	}
	if got := commit("images-disabled", file("screenshot.jpg", jpg.Bytes())); got.SyncDisposition != SyncPolicyFilter {
		t.Fatalf("image policy was bypassed: %+v", got)
	}
	if dir := os.Getenv("UNIC_WXIME_E2E_ARTIFACTS"); dir != "" {
		receipt, err := json.MarshalIndent(map[string]any{
			"real_plugin_subprocess": true, "file_history_preserved": true, "image_policy_used": true,
			"multiple_files_skipped": true, "invalid_image_skipped": true,
			"png_sha256":              fmt.Sprintf("%x", sha256.Sum256(payload)),
			"clipboard_png_preferred": true,
		}, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "file-image.json"), receipt, 0600); err != nil {
			t.Fatal(err)
		}
	}
}
