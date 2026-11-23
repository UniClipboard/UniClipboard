package v1migration

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"maps"
	"os"
	"path/filepath"
	"strings"

	"github.com/uniclipboard/unic/plugin"
)

// tagLayout reads the presentation file once. Custom identities come only from
// authenticated definitions; a display name never identifies a built-in tag.
func (s *Source) tagLayout() (map[string]string, map[string]bool, error) {
	ids := map[string]string{"link": plugin.TagLink, "code": plugin.TagCode, "image": plugin.TagImage, "directory": plugin.TagFolder}
	colors := map[string]string{plugin.TagLink: "blue", plugin.TagCode: "purple", plugin.TagImage: "green", plugin.TagFolder: "gray"}
	path := filepath.Join(s.root, "history-tags", "layout.v1.json")
	if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
		return colors, nil, nil
	}
	data, err := readBounded(path, 1<<20)
	if err != nil {
		return nil, nil, ErrTags
	}
	var doc struct {
		Version int               `json:"version"`
		Sidebar []string          `json:"sidebar"`
		Colors  map[string]string `json:"colors"`
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&doc) != nil || doc.Version != 1 || doc.Colors == nil || doc.Sidebar == nil || decoder.Decode(new(any)) != io.EOF {
		return nil, nil, ErrTags
	}
	custom := map[string]string{}
	for id, color := range doc.Colors {
		if !validTagColor(color) {
			return nil, nil, ErrTags
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
			return nil, nil, ErrTags
		}
		custom[name] = color
	}
	maps.Copy(colors, custom)
	visible := make(map[string]bool, len(doc.Sidebar))
	for _, id := range doc.Sidebar {
		visible[id] = true
	}
	sidebar := make(map[string]bool, len(ids)+len(s.tags))
	for id, name := range ids {
		sidebar[name] = visible[id]
	}
	for id, name := range s.tags {
		sidebar[name] = sidebar[name] || visible[id]
	}
	return colors, sidebar, nil
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
