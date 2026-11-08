// Package settings defines unic's portable plugin-settings schema profile.
// It has no dependency on the engine, a GUI toolkit, or a plugin runtime.
package settings

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"math"
	"sort"
	"strconv"
	"unicode/utf8"
)

const (
	Dialect        = "https://json-schema.org/draft/2020-12/schema"
	Version        = 1
	MaxSchemaBytes = 256 << 10
	MaxConfigBytes = 64 << 10
	MaxDepth       = 16
	MaxNodes       = 512
	MaxSafeNumber  = 9007199254740991
)

// Schema is the supported subset of JSON Schema, with portable UI hints.
// Only the root carries Dialect and Version. Parse rejects other keywords.
type Schema struct {
	Dialect              string             `json:"$schema,omitempty"`
	Version              int                `json:"x-unic-settings-version,omitempty"`
	Type                 string             `json:"type"`
	Title                string             `json:"title,omitempty"`
	Description          string             `json:"description,omitempty"`
	Properties           map[string]*Schema `json:"properties,omitempty"`
	Required             []string           `json:"required,omitempty"`
	AdditionalProperties *bool              `json:"additionalProperties,omitempty"`
	Items                *Schema            `json:"items,omitempty"`
	Enum                 []json.RawMessage  `json:"enum,omitempty"`
	Default              json.RawMessage    `json:"default,omitempty"`
	Minimum              *float64           `json:"minimum,omitempty"`
	Maximum              *float64           `json:"maximum,omitempty"`
	MinLength            *int               `json:"minLength,omitempty"`
	MaxLength            *int               `json:"maxLength,omitempty"`
	MinItems             *int               `json:"minItems,omitempty"`
	MaxItems             *int               `json:"maxItems,omitempty"`
	WriteOnly            bool               `json:"writeOnly,omitempty"`
	UI                   *UI                `json:"x-unic-ui,omitempty"`
}

type UI struct {
	Widget       string   `json:"widget,omitempty"`
	Order        int      `json:"order,omitempty"`
	Group        string   `json:"group,omitempty"`
	OptionLabels []string `json:"option_labels,omitempty"` // parallel to enum
}

// Error never includes a submitted value or an unknown configuration key.
// Path is a JSON pointer containing only declared schema keys/array indices.
type Error struct {
	Code string `json:"code"`
	Path string `json:"path"`
}

func (e *Error) Error() string { return "settings: " + e.Code + " at " + e.Path }

func fail(code, path string) error { return &Error{Code: code, Path: path} }

// Definition owns a parsed schema; it is immutable and safe for concurrent use.
type Definition struct {
	root *Schema
	data []byte
}

// Parse validates a static schema without loading or executing plugin code.
// Unsupported keywords and versions fail closed.
func Parse(data []byte) (*Definition, error) {
	raw, err := decodeJSON(data, MaxSchemaBytes)
	if err != nil {
		return nil, fail("invalid_schema", "")
	}
	if err := checkKeywords(raw, true); err != nil {
		return nil, err
	}
	var s Schema
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if err := d.Decode(&s); err != nil {
		return nil, fail("invalid_schema", "")
	}
	if s.Dialect != Dialect || s.Version != Version || s.Type != "object" {
		return nil, fail("unsupported_schema", "")
	}
	count := 0
	if err := checkSchema(&s, "", 0, &count); err != nil {
		return nil, err
	}
	canonical, err := json.MarshalIndent(&s, "", "  ")
	if err != nil || len(canonical) > MaxSchemaBytes {
		return nil, fail("invalid_schema", "")
	}
	// Check defaults only after the complete tree has been validated.
	if err := checkDefaults(&s, ""); err != nil {
		return nil, err
	}
	return &Definition{root: &s, data: canonical}, nil
}

func checkKeywords(v any, root bool) error {
	fields, ok := v.(map[string]any)
	if !ok {
		return fail("invalid_schema", "")
	}
	for key, value := range fields {
		if key != "default" && value == nil {
			return fail("invalid_schema", "")
		}
		if !root && (key == "$schema" || key == "x-unic-settings-version") {
			return fail("invalid_schema", "")
		}
	}
	if hints, ok := fields["x-unic-ui"].(map[string]any); ok {
		for _, value := range hints {
			if value == nil {
				return fail("invalid_schema", "")
			}
		}
	}
	if children, ok := fields["properties"].(map[string]any); ok {
		for _, child := range children {
			if err := checkKeywords(child, false); err != nil {
				return err
			}
		}
	}
	if child, ok := fields["items"]; ok {
		return checkKeywords(child, false)
	}
	return nil
}

// JSON returns a fresh copy of the canonical schema.
func (d *Definition) JSON() []byte { return bytes.Clone(d.data) }

// Validate checks a complete configuration. Defaults are not inserted.
func (d *Definition) Validate(data []byte) error {
	v, err := decodeJSON(data, MaxConfigBytes)
	if err != nil {
		return fail("invalid_json", "")
	}
	return validate(d.root, v, "")
}

// Normalize copies a configuration, inserts defaults for absent fields, then
// validates it. Explicit false, zero, empty string, and null are never replaced.
// An absent object is not created unless that object has its own default.
func (d *Definition) Normalize(data []byte) ([]byte, error) {
	v, err := decodeJSON(data, MaxConfigBytes)
	if err != nil {
		return nil, fail("invalid_json", "")
	}
	v = defaults(d.root, v)
	if err := validate(d.root, v, ""); err != nil {
		return nil, err
	}
	out, err := json.Marshal(v)
	if err != nil || len(out) > MaxConfigBytes {
		return nil, fail("limit_exceeded", "")
	}
	return out, nil
}

// decodeJSON rejects duplicate keys, invalid UTF-8, trailing JSON, and excessive
// depth before decoding the typed schema/configuration.
func decodeJSON(data []byte, limit int) (any, error) {
	if len(data) > limit || !utf8.Valid(data) {
		return nil, errors.New("invalid JSON")
	}
	d := json.NewDecoder(bytes.NewReader(data))
	d.UseNumber()
	maxDepth := 32
	if limit == MaxSchemaBytes {
		maxDepth = 64 // properties/items add JSON containers to schema depth.
	}
	v, err := readValue(d, 0, maxDepth)
	if err != nil {
		return nil, err
	}
	if _, err := d.Token(); err != io.EOF {
		return nil, errors.New("trailing JSON")
	}
	return v, nil
}

func readValue(d *json.Decoder, depth, maxDepth int) (any, error) {
	if depth > maxDepth {
		return nil, errors.New("JSON depth")
	}
	t, err := d.Token()
	if err != nil {
		return nil, err
	}
	switch t {
	case json.Delim('{'):
		m := map[string]any{}
		for d.More() {
			k, err := d.Token()
			if err != nil {
				return nil, err
			}
			key, ok := k.(string)
			if !ok {
				return nil, errors.New("object key")
			}
			if _, exists := m[key]; exists {
				return nil, errors.New("duplicate key")
			}
			m[key], err = readValue(d, depth+1, maxDepth)
			if err != nil {
				return nil, err
			}
		}
		if _, err := d.Token(); err != nil {
			return nil, err
		}
		return m, nil
	case json.Delim('['):
		a := []any{}
		for d.More() {
			v, err := readValue(d, depth+1, maxDepth)
			if err != nil {
				return nil, err
			}
			a = append(a, v)
		}
		if _, err := d.Token(); err != nil {
			return nil, err
		}
		return a, nil
	}
	if _, ok := t.(json.Delim); ok {
		return nil, errors.New("unexpected delimiter")
	}
	if n, ok := t.(json.Number); ok && !portableNumber(string(n)) {
		return nil, errors.New("nonportable number")
	}
	return t, nil
}

func keys(s *Schema) []string {
	k := make([]string, 0, len(s.Properties))
	for key := range s.Properties {
		k = append(k, key)
	}
	sort.Strings(k)
	return k
}

func checkSchema(s *Schema, path string, depth int, count *int) error {
	if s == nil {
		return fail("invalid_schema", path)
	}
	*count++
	if depth > MaxDepth || *count > MaxNodes {
		return fail("limit_exceeded", path)
	}
	if depth != 0 && (s.Dialect != "" || s.Version != 0) {
		return fail("invalid_schema", path)
	}
	if s.Type != "object" && (s.Properties != nil || s.Required != nil || s.AdditionalProperties != nil) ||
		s.Type != "array" && (s.Items != nil || s.MinItems != nil || s.MaxItems != nil) ||
		s.Type != "string" && (s.MinLength != nil || s.MaxLength != nil || s.WriteOnly) ||
		s.Type != "integer" && s.Type != "number" && (s.Minimum != nil || s.Maximum != nil) {
		return fail("invalid_schema", path)
	}
	switch s.Type {
	case "object":
		if s.AdditionalProperties == nil || *s.AdditionalProperties || s.Enum != nil {
			return fail("invalid_schema", path)
		}
		seen := map[string]bool{}
		for _, k := range s.Required {
			if _, ok := s.Properties[k]; !ok || seen[k] {
				return fail("invalid_schema", path)
			}
			seen[k] = true
		}
		for _, k := range keys(s) {
			if k == "" || len(k) > 128 {
				return fail("invalid_schema", path)
			}
			if err := checkSchema(s.Properties[k], pointer(path, k), depth+1, count); err != nil {
				return err
			}
		}
	case "array":
		if s.Items == nil || s.Items.Type == "object" || s.Items.Type == "array" || s.Enum != nil {
			return fail("unsupported_schema", path)
		}
		if err := checkSchema(s.Items, path, depth+1, count); err != nil {
			return err
		}
	case "string", "boolean", "integer", "number":
	default:
		return fail("unsupported_schema", path)
	}
	if !intRange(s.MinLength, s.MaxLength) || !intRange(s.MinItems, s.MaxItems) {
		return fail("invalid_schema", path)
	}
	for _, n := range []*float64{s.Minimum, s.Maximum} {
		if n != nil && (math.IsNaN(*n) || math.IsInf(*n, 0) || math.Abs(*n) > MaxSafeNumber) {
			return fail("invalid_schema", path)
		}
	}
	if s.Minimum != nil && s.Maximum != nil && *s.Minimum > *s.Maximum {
		return fail("invalid_schema", path)
	}
	if s.WriteOnly && len(s.Default) != 0 {
		return fail("invalid_schema", path)
	}
	if s.Enum != nil {
		if len(s.Enum) == 0 || len(s.Enum) > 128 || s.WriteOnly {
			return fail("invalid_schema", path)
		}
		withoutEnum := *s
		withoutEnum.Enum = nil
		seen := map[string]bool{}
		for _, raw := range s.Enum {
			v, err := decodeJSON(raw, MaxConfigBytes)
			if err != nil || validate(&withoutEnum, v, path) != nil {
				return fail("invalid_schema", path)
			}
			key := enumKey(v)
			if seen[key] {
				return fail("invalid_schema", path)
			}
			seen[key] = true
		}
	}
	if s.UI != nil {
		u := s.UI
		widget := u.Widget
		valid := widget == "" ||
			widget == "input" && s.Type == "string" ||
			widget == "textarea" && s.Type == "string" ||
			widget == "password" && s.WriteOnly ||
			widget == "inputnumber" && (s.Type == "integer" || s.Type == "number") ||
			widget == "switch" && s.Type == "boolean" ||
			widget == "select" && len(s.Enum) > 0 ||
			widget == "group" && s.Type == "object" ||
			widget == "list" && s.Type == "array"
		if !valid || s.WriteOnly && widget != "" && widget != "password" ||
			len(s.Enum) > 0 && widget != "" && widget != "select" ||
			len(u.OptionLabels) > 0 && len(u.OptionLabels) != len(s.Enum) {
			return fail("invalid_schema", path)
		}
	}
	return nil
}

func intRange(min, max *int) bool {
	return (min == nil || *min >= 0) && (max == nil || *max >= 0) &&
		(min == nil || max == nil || *min <= *max)
}

func checkDefaults(s *Schema, path string) error {
	if len(s.Default) > 0 {
		v, err := decodeJSON(s.Default, MaxConfigBytes)
		if err != nil || validate(s, defaults(s, v), path) != nil {
			return fail("invalid_schema", path)
		}
		// Secrets may not be hidden in an ancestor object default.
		if containsSecretValue(s, v) {
			return fail("invalid_schema", path)
		}
	}
	for _, k := range keys(s) {
		if err := checkDefaults(s.Properties[k], pointer(path, k)); err != nil {
			return err
		}
	}
	if s.Items != nil {
		return checkDefaults(s.Items, path)
	}
	return nil
}

func containsSecretValue(s *Schema, v any) bool {
	if s.WriteOnly {
		return true
	}
	if m, ok := v.(map[string]any); ok {
		for k, child := range s.Properties {
			if value, exists := m[k]; exists && containsSecretValue(child, value) {
				return true
			}
		}
	}
	if a, ok := v.([]any); ok && s.Items != nil {
		for _, value := range a {
			if containsSecretValue(s.Items, value) {
				return true
			}
		}
	}
	return false
}

func enumKey(v any) string {
	if n, ok := v.(json.Number); ok {
		f, _ := n.Float64()
		return "number:" + strconv.FormatFloat(f, 'g', -1, 64)
	}
	b, _ := json.Marshal(v)
	return string(b)
}
