package diaglogs

import (
	"archive/zip"
	_ "embed"
	"encoding/json"
)

//go:embed import/collector.yaml
var collectorConfig []byte

//go:embed import/README.md
var importReadme []byte

//go:embed import/import.py
var importScript []byte

// collectOfflineOTel never reads keys or starts the daemon. In-memory histories cannot be recovered offline.
func collectOfflineOTel(archive *zip.Writer) error {
	files := []any{}

	summary, err := json.MarshalIndent(map[string]any{
		"schemaVersion": 1, "format": "OTLP JSONL", "collectorVersion": "0.139.0",
		"liveFlush": false, "state": "unavailable", "reason": "daemon_not_running",
		"history": nil, "files": files,
	}, "", "  ")
	if err != nil {
		return err
	}
	for _, item := range []struct {
		name    string
		content []byte
	}{
		{"otel/manifest.json", summary}, {"import/collector.yaml", collectorConfig}, {"import/README.md", importReadme}, {"import/import.py", importScript},
	} {
		out, err := archive.CreateHeader(&zip.FileHeader{Name: item.name, Method: zip.Deflate})
		if err != nil {
			return err
		}
		if _, err := out.Write(item.content); err != nil {
			return err
		}
	}
	return nil
}
