package luascripts

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/settings"
)

type savedSettings struct {
	Schema   string          `json:"schema_sha256"`
	Revision string          `json:"revision"`
	Values   json.RawMessage `json:"values"`
	Secrets  []string        `json:"secret_paths,omitempty"`
}

func (s Script) ReadSettings(root string) (string, []byte, error) {
	path := filepath.Join(filepath.Dir(root), "settings", s.settingsID()+".json")
	if _, err := os.Lstat(path); errors.Is(err, os.ErrNotExist) {
		return schemaDigest(s.Schema), s.InitialSettings, nil
	}
	dir, err := os.OpenRoot(filepath.Dir(path))
	if err != nil {
		return "", nil, errors.New("plugin settings unavailable")
	}
	defer func() { _ = dir.Close() }()
	raw, err := readFile(dir, filepath.Base(path), settings.MaxConfigBytes+settings.MaxSchemaBytes)
	var saved savedSettings
	if err != nil || json.Unmarshal(raw, &saved) != nil || !hashPattern.MatchString(saved.Revision) || !hashPattern.MatchString(saved.Schema) {
		return "", nil, errors.New("plugin settings unavailable")
	}
	if saved.Schema != schemaDigest(s.Schema) {
		var tree settings.Schema
		values, err := settings.DecodeValues(saved.Values)
		if err != nil || json.Unmarshal(s.Schema.JSON(), &tree) != nil {
			return "", nil, errors.New("plugin settings unavailable")
		}
		priorSecrets := make(map[string]bool)
		for _, path := range saved.Secrets {
			priorSecrets[path] = true
		}
		values = filterPreviousSettings(&tree, values, "", priorSecrets)
		saved.Values, err = json.Marshal(values)
		if err != nil {
			return "", nil, errors.New("plugin settings unavailable")
		}
	}
	if values, err := s.Schema.Normalize(saved.Values); err == nil {
		saved.Values = values
	}
	return schemaDigest(s.Schema) + "." + saved.Revision, saved.Values, nil
}

func (s Script) RuntimeConfig(root string, fallback map[string]any) (plugin.Config, error) {
	cfg := make(plugin.Config)
	if s.Schema == nil {
		for key, v := range s.Manifest.Defaults {
			cfg[key] = v
		}
		for key, v := range fallback {
			cfg[key] = v
		}
		return cfg, nil
	}
	_, data, err := s.ReadSettings(root)
	if err != nil {
		return nil, err
	}
	data, err = s.Schema.Normalize(data)
	if err != nil {
		return nil, err
	}
	err = json.Unmarshal(data, &cfg)
	return cfg, err
}

func (s Script) SettingsView(revision string, data []byte) (api.PluginSettings, error) {
	schema := s.Schema.JSON()
	var tree settings.Schema
	var values map[string]any
	if json.Unmarshal(schema, &tree) != nil || decodeValues(data, &values) != nil {
		return api.PluginSettings{}, errors.New("plugin settings unavailable")
	}
	secrets := make(map[string]bool)
	publicValues(&tree, values, "", secrets)
	raw, err := json.Marshal(values)
	return api.PluginSettings{Revision: revision, Schema: schema, Values: raw, Secrets: secrets}, err
}

// Only carry forward values valid for the new field. A former secret
// may be carried forward only if it is still classified as write-only.
func filterPreviousSettings(schema *settings.Schema, old map[string]any, parent string, secrets map[string]bool) map[string]any {
	out := make(map[string]any)
	for key, field := range schema.Properties {
		value, exists := old[key]
		path := fieldPath(parent, key)
		if !exists || secrets[path] && !field.WriteOnly {
			continue
		}
		if field.Type == "object" {
			if child, ok := value.(map[string]any); ok {
				out[key] = filterPreviousSettings(field, child, path, secrets)
			}
		} else if settings.ValidateField(field, value) == nil {
			out[key] = value
		}
	}
	return out
}

func publicValues(schema *settings.Schema, values map[string]any, parent string, secrets map[string]bool) {
	for key := range values {
		if _, known := schema.Properties[key]; !known {
			delete(values, key)
		}
	}
	for key, field := range schema.Properties {
		path := fieldPath(parent, key)
		if field.WriteOnly {
			_, secrets[path] = values[key]
			delete(values, key)
		} else if field.Type == "object" {
			child, ok := values[key].(map[string]any)
			if !ok {
				delete(values, key)
			}
			publicValues(field, child, path, secrets)
		}
	}
}

func fieldPath(parent, key string) string {
	return parent + "/" + strings.ReplaceAll(strings.ReplaceAll(key, "~", "~0"), "/", "~1")
}

func decodeValues(data []byte, out *map[string]any) error {
	value, err := settings.DecodeValues(data)
	if err != nil {
		return err
	}
	*out = value
	return nil
}

func (s Script) EditSettings(current, input []byte) ([]byte, error) {
	var tree settings.Schema
	var old, values map[string]any
	if err := json.Unmarshal(s.Schema.JSON(), &tree); err != nil {
		return nil, err
	}
	if err := decodeValues(current, &old); err != nil {
		return nil, err
	}
	if err := decodeValues(input, &values); err != nil {
		return nil, err
	}
	preserveSecrets(&tree, old, values)
	raw, err := json.Marshal(values)
	if err != nil {
		return nil, err
	}
	return s.Schema.Normalize(raw)
}

func preserveSecrets(schema *settings.Schema, old, values map[string]any) {
	for key, field := range schema.Properties {
		value, present := values[key]
		if field.WriteOnly {
			switch {
			case !present:
				if previous, ok := old[key]; ok {
					values[key] = previous
				}
			case value == nil:
				delete(values, key)
			}
		} else if field.Type == "object" {
			previous, _ := old[key].(map[string]any)
			child, ok := value.(map[string]any)
			if !present {
				child = make(map[string]any)
			}
			if ok || !present {
				preserveSecrets(field, previous, child)
				if !present && len(child) != 0 {
					values[key] = child
				}
			}
		}
	}
}

func (s Script) WriteSettings(root string, values []byte) (string, error) {
	var random [32]byte
	if _, err := rand.Read(random[:]); err != nil {
		return "", err
	}
	revision := hex.EncodeToString(random[:])
	view, err := s.SettingsView(revision, values)
	if err != nil {
		return "", err
	}
	var secrets []string
	for path, set := range view.Secrets {
		if set {
			secrets = append(secrets, path)
		}
	}
	slices.Sort(secrets)
	data, err := json.Marshal(savedSettings{Schema: schemaDigest(s.Schema), Revision: revision, Values: values, Secrets: secrets})
	if err != nil {
		return "", err
	}
	dir := filepath.Join(filepath.Dir(root), "settings")
	if err := os.MkdirAll(dir, 0700); err != nil {
		return "", err
	}
	if err := atomicWrite(dir, s.settingsID()+".json", data); err != nil {
		return "", err
	}
	return schemaDigest(s.Schema) + "." + revision, nil
}

func atomicWrite(dir, name string, data []byte) error {
	f, err := os.CreateTemp(dir, ".save-*")
	if err != nil {
		return err
	}
	defer func() { _ = os.Remove(f.Name()) }()
	_, writeErr := f.Write(data)
	if writeErr == nil {
		writeErr = f.Sync()
	}
	if err := errors.Join(writeErr, f.Close()); err != nil {
		return err
	}
	return os.Rename(f.Name(), filepath.Join(dir, name))
}
