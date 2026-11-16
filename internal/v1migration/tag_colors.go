package v1migration

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"maps"
	"os"
	"path/filepath"
	"strings"

	"github.com/BurntSushi/toml"

	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/plugin"
)

// tagColors reads the presentation file once. Custom identities come only from
// authenticated definitions; a display name never identifies a built-in tag.
func (s *Source) tagColors() (map[string]string, error) {
	ids := map[string]string{"link": plugin.TagLink, "code": plugin.TagCode, "image": plugin.TagImage, "directory": plugin.TagFolder}
	colors := map[string]string{plugin.TagLink: "blue", plugin.TagCode: "purple", plugin.TagImage: "green", plugin.TagFolder: "gray"}
	path := filepath.Join(s.root, "history-tags", "layout.v1.json")
	if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
		return colors, nil
	}
	data, err := readBounded(path, 1<<20)
	if err != nil {
		return nil, ErrTags
	}
	var doc struct {
		Version int               `json:"version"`
		Sidebar []string          `json:"sidebar"`
		Colors  map[string]string `json:"colors"`
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&doc) != nil || doc.Version != 1 || doc.Colors == nil || doc.Sidebar == nil || decoder.Decode(new(any)) != io.EOF {
		return nil, ErrTags
	}
	custom := map[string]string{}
	for id, color := range doc.Colors {
		if !validTagColor(color) {
			return nil, ErrTags
		}
		color = strings.ToLower(color)
		if builtin, ok := ids[id]; ok {
			colors[builtin] = color
			continue
		}
		name, ok := s.tags[id]
		if !ok {
			continue
		} // v1 layouts can retain deleted custom IDs until pruning.
		if old, exists := custom[name]; exists && old != color {
			return nil, ErrTags
		}
		custom[name] = color
	}
	maps.Copy(colors, custom)
	return colors, nil
}

func validTagColor(color string) bool {
	switch color {
	case "orange", "blue", "green", "purple", "gray":
		return true
	}
	if len(color) != 7 || color[0] != '#' {
		return false
	}
	_, err := hex.DecodeString(color[1:])
	return err == nil
}

func migrationColorKey(id string) string {
	sum := sha256.Sum256([]byte(id))
	return hex.EncodeToString(sum[:])
}

// importTagColors uses the host's existing config CAS while Run owns the target
// directory lock. Existing target overrides always win, including explicit gray.
func importTagColors(ctx context.Context, target string, colors map[string]string, write bool, report *Report) {
	path := filepath.Join(target, "config.toml")
	cfg := config.Default()
	var manager *config.Manager
	var err error
	if write {
		manager, err = config.Open(path, nil)
		if err == nil {
			cfg = manager.Current()
		}
	} else {
		var raw []byte
		raw, err = os.ReadFile(path)
		if err == nil {
			cfg, err = config.Parse(raw)
		} else if errors.Is(err, os.ErrNotExist) {
			err = nil
		}
	}
	if err != nil {
		report.ColorFailures++
		return
	}
	cfg.Desktop.TagColors = maps.Clone(cfg.Desktop.TagColors)
	if cfg.Desktop.TagColors == nil {
		cfg.Desktop.TagColors = map[string]string{}
	}
	expected := map[string]string{}
	missing := 0
	for id, color := range colors {
		key := migrationColorKey(id)
		if current, ok := cfg.Desktop.TagColors[key]; ok {
			report.ColorsPreserved++
			expected[key] = current
			continue
		}
		missing++
		if write {
			cfg.Desktop.TagColors[key] = color
			expected[key] = color
		}
	}
	if !write {
		report.ColorsMissing = missing
	}
	if write && missing > 0 {
		if ctx.Err() != nil {
			report.ColorFailures++
			return
		}
		var encoded bytes.Buffer
		if toml.NewEncoder(&encoded).Encode(cfg) != nil {
			report.ColorFailures++
			return
		}
		if _, replaceErr := manager.Replace(manager.State().Revision, encoded.Bytes()); replaceErr != nil {
			report.ColorFailures++
			return
		}
		report.ColorsImported = missing
	}
	// Reopen persisted configuration rather than trusting the in-memory candidate.
	persisted, readErr := os.ReadFile(path)
	if errors.Is(readErr, os.ErrNotExist) && len(expected) == 0 {
		return
	}
	if readErr != nil {
		report.ColorFailures++
		return
	}
	reopened, parseErr := config.Parse(persisted)
	if parseErr != nil {
		report.ColorFailures++
		return
	}
	for key, want := range expected {
		if reopened.Desktop.TagColors[key] != want {
			report.ColorFailures++
			continue
		}
		report.ColorsVerified++
	}
}
