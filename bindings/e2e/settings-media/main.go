// Command settings-media exercises the mobile bridge against a disposable
// profile across independent processes.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"

	"github.com/uniclipboard/unic/bindings/mobile"
	"github.com/uniclipboard/unic/engine"
)

const payload = "Mobile file export fixture\n"

type fixture struct {
	Settings engine.Settings `json:"settings"`
	File     string          `json:"file"`
	Image    string          `json:"image"`
	Folder   string          `json:"folder"`
}

type receipt struct {
	Checks      []string `json:"checks"`
	ImageSHA256 string   `json:"image_sha256"`
	FileSHA256  string   `json:"file_sha256"`
}

func must[T any](result T, err error) T {
	check(err)
	return result
}

func check(err error) {
	if err != nil {
		panic(err)
	}
}

func require(condition bool, message string) {
	if !condition {
		panic(message)
	}
}

func encode(value any) string { return string(must(json.Marshal(value))) }

func decode[T any](raw string) T {
	var result T
	check(json.Unmarshal([]byte(raw), &result))
	return result
}

func sha(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func pngFixture() []byte {
	var output bytes.Buffer
	img := image.NewRGBA(image.Rect(0, 0, 2, 2))
	img.Set(0, 0, color.RGBA{R: 255, A: 255})
	check(png.Encode(&output, img))
	return output.Bytes()
}

func openMobile(dir string) *mobile.Engine {
	m := must(mobile.NewEngine(encode(map[string]string{
		"data_dir":  filepath.Join(dir, "profile"),
		"cache_dir": filepath.Join(dir, "cache"),
	})))
	check(m.Start(nil))
	return m
}

func commit(m *mobile.Engine, request, handleJSON string) string {
	handle := decode[engine.ContentHandle](handleJSON)
	defer m.ReleaseContent(string(handle.LeaseID))
	result := decode[engine.SubmissionResult](must(m.CommitContent(nil, encode(map[string]string{
		"request_id": request, "lease_id": string(handle.LeaseID),
	}))))
	require(result.HistoryID != "", "commit did not create history")
	return string(result.HistoryID)
}

func writeProfile(dir string) {
	m := openMobile(dir)
	initial := decode[engine.State](must(m.GetState()))
	settings := initial.Settings.Value
	settings.DeviceName = "Mobile M2 Fixture"
	settings.RendezvousURL = "https://example.invalid"
	settings.History = engine.HistoryCleanupSettings{
		AutoCleanup: false, RetentionDays: 7, MaxItems: 99, ExcludedTags: []string{"pinned", "starred"},
	}
	settings.Sync.Enabled = false
	settings.Sync.Automatic = false
	settings.Sync.OnRestore = true
	settings.Sync.Files = false
	settings.Sync.Introductions = false
	settings.Limits.MaxFileBytes = 1 << 20
	settings.Limits.MaxTotalBytes = 8 << 20
	settings.FileCache.MaxBytes = 0

	request := map[string]any{"expected_revision": strconv.FormatUint(initial.Settings.Revision, 10), "value": settings}
	saved := decode[engine.SettingsState](must(m.UpdateSettings(encode(request))))
	_, err := m.UpdateSettings(encode(request))
	require(err != nil && strings.Contains(err.Error(), `"code":"conflict"`), "stale settings revision was accepted")
	require(reflect.DeepEqual(settings, saved.Value), "effective settings differ from saved settings")

	f := fixture{
		Settings: saved.Value,
		File:     commit(m, "m2-file", must(m.ImportBytes("text/plain", "report.txt", []byte(payload)))),
		Image:    commit(m, "m2-image", must(m.ImportBytes(engine.MimePNG, "", pngFixture()))),
	}
	check(m.Close(nil))
	f.Folder = seedDirectory(dir)
	check(os.WriteFile(filepath.Join(dir, "fixture.json"), []byte(encode(f)), 0600))
}

func seedDirectory(dir string) string {
	ctx := context.Background()
	core := must(engine.New(ctx, engine.Options{
		DataDir:  filepath.Join(dir, "profile"),
		CacheDir: filepath.Join(dir, "cache"),
		Settings: engine.DefaultSettings(),
	}))
	defer core.Close(ctx)
	check(core.Start(ctx))
	manifest := encode(engine.DirectoryManifest{
		Roots: []engine.DirectoryRoot{{Name: "reports", Directory: true}},
		Entries: []engine.DirectoryManifestEntry{
			{Directory: true},
			{Path: "report.txt", Size: int64(len(payload)), SHA256: sha([]byte(payload))},
		},
	})
	handle := must(core.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{
			MIME: engine.MimeDirectory, Size: int64(len(manifest)), Reader: strings.NewReader(manifest),
		}},
		Files: []engine.FileInput{{
			Name: "report.txt", MIME: "text/plain", Size: int64(len(payload)), Reader: strings.NewReader(payload),
		}},
	}))
	defer core.ReleaseContent(ctx, handle.LeaseID)
	result := must(core.CommitContent(ctx, engine.CommitRequest{RequestID: "m2-folder", LeaseID: handle.LeaseID}))
	require(result.HistoryID != "", "folder fixture was not persisted")
	return string(result.HistoryID)
}

func verifyProfile(dir string) {
	f := decode[fixture](string(must(os.ReadFile(filepath.Join(dir, "fixture.json")))))
	m := openMobile(dir)
	defer m.Close(nil)
	current := decode[engine.State](must(m.GetState()))
	require(reflect.DeepEqual(current.Settings.Value, f.Settings), "settings changed after process restart")
	require(current.Capabilities.Effective.Observe == engine.AbilityUnavailable, "clipboard capability was not reported")
	cache := decode[engine.FileCacheState](must(m.GetFileCacheState()))
	require(cache.StoredBytes == 0 && cache.ReservedBytes == 0, "fixture unexpectedly uses received file cache")

	meta := decode[[]engine.PartDescriptor](must(m.HistoryFileMetadata(f.File)))
	require(len(meta) == 1 && meta[0].Name == "report.txt" && meta[0].MIME == "text/plain" &&
		meta[0].Size == int64(len(payload)), "file metadata differs from original")
	type mediaFile struct {
		Name string `json:"name"`
		Path string `json:"path"`
	}
	type export struct {
		Directory string      `json:"directory"`
		Files     []mediaFile `json:"files"`
	}
	file := decode[export](must(m.StageHistoryExport(f.File, false)))
	require(len(file.Files) == 1 && file.Files[0].Name == "report.txt", "export did not preserve the filename")
	fileBytes := must(os.ReadFile(file.Files[0].Path))
	require(bytes.Equal(fileBytes, []byte(payload)), "file export bytes changed")

	img := decode[export](must(m.StageHistoryExport(f.Image, true)))
	require(len(img.Files) == 1 && filepath.Ext(img.Files[0].Name) == ".png", "photo export did not preserve PNG format")
	imgBytes := must(os.ReadFile(img.Files[0].Path))
	require(bytes.Equal(imgBytes, pngFixture()), "photo export bytes changed")

	type folderListing struct {
		Entries []struct {
			Path      string `json:"path"`
			Directory bool   `json:"directory"`
			Size      int64  `json:"size"`
		}
		Truncated bool
	}
	folder := decode[folderListing](must(m.ListHistoryFolder(f.Folder, 200)))
	require(len(folder.Entries) == 2 && folder.Entries[0].Directory && !folder.Truncated &&
		folder.Entries[1].Path == "reports/report.txt" && folder.Entries[1].Size == int64(len(payload)),
		"folder listing did not preserve nested path and size")
	bounded := decode[folderListing](must(m.ListHistoryFolder(f.Folder, 1)))
	require(len(bounded.Entries) == 1 && bounded.Truncated, "folder limit did not indicate truncation")

	preview := decode[mediaFile](must(m.ExportHistoryImagePreview(f.Image)))
	require(filepath.Ext(preview.Path) == ".png", "preview lacks its media extension")
	check(m.ClearHistoryPreviews())
	require(os.IsNotExist(fileError(preview.Path)), "preview was not cleared")
	preview = decode[mediaFile](must(m.ExportHistoryImagePreview(f.Image)))
	item := decode[engine.HistoryItem](must(m.GetHistoryItem(f.Image)))
	check(m.DeleteHistoryItem(f.Image, strconv.FormatUint(item.Revision, 10)))
	check(m.ReconcileHistoryPreviews())
	require(os.IsNotExist(fileError(preview.Path)), "deleted history left an orphaned preview")

	fmt.Println(encode(receipt{
		Checks: []string{"settings-conflict", "settings-process-restart", "capabilities", "file-cache-usage",
			"file-metadata", "file-export-bytes", "photo-export-bytes", "folder-listing", "folder-limit", "preview-cleanup"},
		FileSHA256: sha(fileBytes), ImageSHA256: sha(imgBytes),
	}))
}

func fileError(path string) error {
	_, err := os.Stat(path)
	return err
}

func main() {
	if len(os.Args) == 3 {
		if os.Args[1] == "write" {
			writeProfile(os.Args[2])
		} else {
			verifyProfile(os.Args[2])
		}
		return
	}
	dir := must(os.MkdirTemp("", "unic-mobile-m2-"))
	defer os.RemoveAll(dir)
	dir = must(filepath.EvalSymlinks(dir))
	binary := must(os.Executable())
	for _, phase := range []string{"write", "read"} {
		command := exec.Command(binary, phase, dir)
		command.Stdout, command.Stderr = os.Stdout, os.Stderr
		check(command.Run())
	}
}
