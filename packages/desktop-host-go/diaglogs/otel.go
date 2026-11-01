package diaglogs

import (
	"archive/zip"
	"crypto/sha256"
	_ "embed"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

//go:embed import/collector.yaml
var collectorConfig []byte

//go:embed import/README.md
var importReadme []byte

//go:embed import/import.py
var importScript []byte

// collectOfflineOTel never reads keys or starts the daemon. In-memory histories cannot be recovered offline.
func collectOfflineOTel(archive *zip.Writer, logsDir string) (bool, error) {
	files := []any{}
	state, reason := "unavailable", "engine_offline_key_unavailable"
	if input, err := openRegular(filepath.Join(logsDir, "otel-history", "history.enc")); err == nil {
		defer input.Close()
		info, err := input.Stat()
		if err == nil && info.Size() <= 8*1024*1024 {
			content, err := io.ReadAll(io.LimitReader(input, info.Size()))
			if err == nil && int64(len(content)) == info.Size() {
				out, err := archive.Create("otel/history.enc")
				if err != nil {
					return false, err
				}
				if _, err := out.Write(content); err != nil {
					return false, err
				}
				files = append(files, map[string]any{"name": "otel/history.enc", "bytes": len(content), "sha256": fmt.Sprintf("%x", sha256.Sum256(content)), "state": "encrypted_unavailable"})
				state, reason = "encrypted_unavailable", "engine_offline_key_unavailable"
			} else {
				reason = "history_unreadable"
			}
		} else {
			reason = "history_unreadable_or_oversized"
		}
	} else if !os.IsNotExist(err) {
		reason = "history_unreadable"
	}

	summary, err := json.MarshalIndent(map[string]any{
		"schemaVersion": 1, "format": "OTLP JSONL", "collectorVersion": "0.139.0",
		"liveFlush": false, "state": state, "reason": reason,
		"history": nil, "files": files,
	}, "", "  ")
	if err != nil {
		return false, err
	}
	for _, item := range []struct {
		name    string
		content []byte
	}{
		{"otel/manifest.json", summary}, {"import/collector.yaml", collectorConfig}, {"import/README.md", importReadme}, {"import/import.py", importScript},
	} {
		out, err := archive.CreateHeader(&zip.FileHeader{Name: item.name, Method: zip.Deflate})
		if err != nil {
			return false, err
		}
		if _, err := out.Write(item.content); err != nil {
			return false, err
		}
	}
	return len(files) > 0, nil
}

func hasOfflineHistory(logsDir string) bool {
	input, err := openRegular(filepath.Join(logsDir, "otel-history", "history.enc"))
	if err != nil {
		return false
	}
	defer input.Close()
	info, err := input.Stat()
	return err == nil && info.Size() > 0 && info.Size() <= 8*1024*1024
}
