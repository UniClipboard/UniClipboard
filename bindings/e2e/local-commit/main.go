// Command local-commit verifies explicit local saves through the mobile binding
// and reopens the profile in a second process.
package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"

	"github.com/uniclipboard/unic/bindings/mobile"
	"github.com/uniclipboard/unic/engine"
)

func must[T any](value T, err error) T {
	check(err)
	return value
}

func check(err error) {
	if err != nil {
		panic(err)
	}
}

func require(condition bool, detail string) {
	if !condition {
		panic(detail)
	}
}

func encode(value any) string {
	return string(must(json.Marshal(value)))
}

func decode[T any](value string) T {
	var result T
	check(json.Unmarshal([]byte(value), &result))
	return result
}

func errorCode(err error) string {
	if err == nil {
		return ""
	}
	var result struct {
		Code string `json:"code"`
	}
	check(json.Unmarshal([]byte(err.Error()), &result))
	return result.Code
}

func openProfile(dir string) *mobile.Engine {
	m := must(mobile.NewEngine(encode(map[string]string{"data_dir": dir})))
	check(m.Start(nil))
	return m
}

func write(dir string) {
	m := openProfile(dir)
	defer m.Close(nil)
	state := decode[engine.State](must(m.GetState()))
	require(state.Settings.Value.Sync.Enabled, "sync must be enabled in the fixture")
	lease := decode[engine.ContentHandle](must(m.ImportText("local commit fixture", "", "")))
	defer m.ReleaseContent(string(lease.LeaseID))
	request := map[string]any{"request_id": "m3-local-save", "lease_id": lease.LeaseID, "local_only": true}
	result := decode[engine.SubmissionResult](must(m.CommitContent(nil, encode(request))))
	require(result.SyncDisposition == engine.SyncNotRequested, "local-only request reached network dispatch: "+string(result.SyncDisposition))
	require(result.HistoryID != "" && result.OperationID == "", "local-only request must commit history and return no send operation")
	retry := decode[engine.SubmissionResult](must(m.CommitContent(nil, encode(request))))
	require(retry.HistoryID == result.HistoryID, "local-only replay wrote a second history item")
	delete(request, "local_only")
	_, err := m.CommitContent(nil, encode(request))
	require(errorCode(err) == "conflict", "different sync modes must conflict under one request ID")
	request["request_id"] = "m3-incompatible-target"
	request["local_only"] = true
	request["device_ids"] = []string{"target"}
	_, err = m.CommitContent(nil, encode(request))
	require(errorCode(err) == "invalid_argument", "local-only must reject target selection")
	verifyShares(m, dir)
}

func verifyShares(m *mobile.Engine, dir string) {
	staging := filepath.Join(filepath.Dir(dir), "shares")
	check(os.MkdirAll(staging, 0o700))
	var pngPayload bytes.Buffer
	img := image.NewRGBA(image.Rect(0, 0, 2, 2))
	img.SetRGBA(0, 0, color.RGBA{R: 255, A: 255})
	check(png.Encode(&pngPayload, img))
	items := []struct {
		id      string
		entry   map[string]string
		payload []byte
	}{
		{"shared-text", map[string]string{"kind": "text", "text": "shared text fixture"}, nil},
		{"shared-photo", map[string]string{"kind": "file", "name": "photo.png", "mime": "image/png"}, pngPayload.Bytes()},
		{"shared-document", map[string]string{"kind": "file", "name": "fixture.txt", "mime": "text/plain"}, []byte("shared file fixture\n")},
	}
	for _, item := range items {
		path := filepath.Join(staging, item.id)
		check(os.Mkdir(path, 0o700))
		check(os.WriteFile(filepath.Join(path, "entry.json"), []byte(encode(item.entry)), 0o600))
		if item.payload != nil {
			check(os.WriteFile(filepath.Join(path, "payload"), item.payload, 0o600))
		}
		result := decode[engine.SubmissionResult](must(m.CommitSharedItem(path, "")))
		require(result.HistoryID != "" && result.OperationID == "" && result.SyncDisposition == engine.SyncNotRequested,
			"sharing local content must create history without dispatch")
		retry := decode[engine.SubmissionResult](must(m.CommitSharedItem(path, "")))
		require(retry.HistoryID == result.HistoryID, "shared item replay wrote a second history item")
	}
}

func read(dir string) {
	m := openProfile(dir)
	defer m.Close(nil)
	page := decode[engine.HistoryPage](must(m.ListHistory(`{"limit":10}`)))
	require(len(page.Items) == 4, "reopened profile must contain four local saves")
	kinds := map[engine.HistoryKind]int{}
	for _, item := range page.Items {
		require(item.SyncState == engine.HistorySyncNotRequested, "reopened history must show sync was not requested")
		kinds[item.Kind]++
	}
	require(kinds[engine.HistoryText] == 2 && kinds[engine.HistoryImage] == 1 && kinds[engine.HistoryFiles] == 1,
		"the shared text, PNG and file must use their normal history kinds")
	for _, item := range page.Items {
		var fixture string
		switch item.Kind {
		case engine.HistoryImage:
			fixture = "shared-photo"
		case engine.HistoryFiles:
			fixture = "shared-document"
		default:
			continue
		}
		export := decode[struct {
			Directory string `json:"directory"`
			Files     []struct {
				Name string `json:"name"`
				Path string `json:"path"`
			} `json:"files"`
		}](must(m.StageHistoryExport(string(item.ID), item.Kind == engine.HistoryImage)))
		defer os.RemoveAll(export.Directory)
		require(len(export.Files) == 1, "each fixture must export one file")
		original := must(os.ReadFile(filepath.Join(filepath.Dir(dir), "shares", fixture, "payload")))
		require(bytes.Equal(original, must(os.ReadFile(export.Files[0].Path))), "shared payload bytes changed across process restart")
		if item.Kind == engine.HistoryFiles {
			require(export.Files[0].Name == "fixture.txt", "the original shared filename must survive restart")
		}
	}
}

func main() {
	if len(os.Args) == 3 {
		switch os.Args[1] {
		case "write":
			write(os.Args[2])
		case "read":
			read(os.Args[2])
		default:
			panic("unknown process role")
		}
		return
	}
	dir := must(os.MkdirTemp("", "unic-m3-local-commit-"))
	defer os.RemoveAll(dir)
	profile := filepath.Join(must(filepath.EvalSymlinks(dir)), "profile")
	binary := must(os.Executable())
	for _, role := range []string{"write", "read"} {
		cmd := exec.Command(binary, role, profile)
		cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		check(cmd.Run())
	}
	fmt.Println(`{"sync_enabled":true,"sync_disposition":"not_requested","send_operation":false,"same_request":"idempotent","mode_change":"conflict","explicit_targets":"invalid_argument","share_local_save":["text","image/png","file"],"reopened_history_items":4,"exported_file_and_png_bytes":"identical"}`)
}
