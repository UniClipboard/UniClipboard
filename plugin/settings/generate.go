package settings

import (
	"encoding/json"
	"reflect"
	"strconv"
	"strings"
)

// Generate derives a schema from a struct type, never from current values.
// Each exported field needs an explicit json name (or json:"-"). Embedding,
// pointers, maps, interfaces, custom JSON marshalers and recursive types are
// rejected so the generated wire shape cannot silently differ from the type.
// See docs/design/plugin-settings-contract.md for the supported field tags.
func Generate[T any]() ([]byte, error) {
	s, err := generate(reflect.TypeFor[T](), map[reflect.Type]bool{}, 0)
	if err != nil {
		return nil, err
	}
	s.Dialect, s.Version = Dialect, Version
	raw, err := json.Marshal(s)
	if err != nil {
		return nil, fail("invalid_schema", "")
	}
	d, err := Parse(raw)
	if err != nil {
		return nil, err
	}
	return d.JSON(), nil
}

func generate(t reflect.Type, seen map[reflect.Type]bool, depth int) (*Schema, error) {
	if depth > MaxDepth || seen[t] {
		return nil, fail("unsupported_type", "")
	}
	marshaler := reflect.TypeFor[json.Marshaler]()
	textMarshaler := reflect.TypeFor[interface{ MarshalText() ([]byte, error) }]()
	if t.Implements(marshaler) || reflect.PointerTo(t).Implements(marshaler) ||
		t.Implements(textMarshaler) || reflect.PointerTo(t).Implements(textMarshaler) {
		return nil, fail("unsupported_type", "")
	}
	s := &Schema{}
	switch t.Kind() {
	case reflect.Struct:
		seen[t] = true
		defer delete(seen, t)
		s.Type = "object"
		no := false
		s.AdditionalProperties = &no
		s.Properties = map[string]*Schema{}
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			if !f.IsExported() {
				continue
			}
			parts := strings.Split(f.Tag.Get("json"), ",")
			if parts[0] == "-" {
				continue
			}
			if f.Anonymous || parts[0] == "" {
				return nil, fail("unsupported_type", "")
			}
			for _, option := range parts[1:] {
				if option != "omitempty" && option != "omitzero" {
					return nil, fail("unsupported_type", "")
				}
			}
			name := parts[0]
			if _, exists := s.Properties[name]; exists {
				return nil, fail("invalid_schema", "")
			}
			child, err := generate(f.Type, seen, depth+1)
			if err != nil {
				return nil, err
			}
			if err := applyTags(child, f.Tag); err != nil {
				return nil, err
			}
			s.Properties[name] = child
			if required, ok := f.Tag.Lookup("required"); ok {
				if required != "true" && required != "false" {
					return nil, fail("invalid_schema", "")
				}
				if required == "true" {
					s.Required = append(s.Required, name)
				}
			}
		}
	case reflect.Slice:
		s.Type = "array"
		var err error
		s.Items, err = generate(t.Elem(), seen, depth+1)
		if err != nil {
			return nil, err
		}
	case reflect.String:
		s.Type = "string"
	case reflect.Bool:
		s.Type = "boolean"
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64,
		reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		s.Type = "integer"
		bits := t.Bits()
		var min, max float64
		if t.Kind() >= reflect.Uint && t.Kind() <= reflect.Uint64 {
			max = minFloat(float64(^uint64(0)>>(64-bits)), MaxSafeNumber)
		} else {
			max = minFloat(float64(uint64(1)<<(bits-1)-1), MaxSafeNumber)
			min = -max - 1
			if min < -MaxSafeNumber {
				min = -MaxSafeNumber
			}
		}
		s.Minimum, s.Maximum = &min, &max
	case reflect.Float64:
		s.Type = "number"
	default: // float32 cannot round-trip all profile numbers; []byte uses base64.
		return nil, fail("unsupported_type", "")
	}
	if t.Kind() == reflect.Slice && t.Elem().Kind() == reflect.Uint8 {
		return nil, fail("unsupported_type", "")
	}
	return s, nil
}

func minFloat(a, b float64) float64 {
	if a < b {
		return a
	}
	return b
}

func applyTags(s *Schema, tags reflect.StructTag) error {
	s.Title, s.Description = tags.Get("title"), tags.Get("description")
	s.UI = &UI{Widget: tags.Get("widget"), Group: tags.Get("group")}
	for _, tag := range []struct {
		name string
		dest **int
	}{{"minLength", &s.MinLength}, {"maxLength", &s.MaxLength}, {"minItems", &s.MinItems}, {"maxItems", &s.MaxItems}} {
		if text, ok := tags.Lookup(tag.name); ok {
			n, err := strconv.Atoi(text)
			if err != nil {
				return fail("invalid_schema", "")
			}
			*tag.dest = &n
		}
	}
	for _, tag := range []struct {
		name string
		dest **float64
	}{{"minimum", &s.Minimum}, {"maximum", &s.Maximum}} {
		if text, ok := tags.Lookup(tag.name); ok {
			n, err := strconv.ParseFloat(text, 64)
			if err != nil {
				return fail("invalid_schema", "")
			}
			// A tag may narrow the Go type's range, never widen it.
			if *tag.dest != nil && (tag.name == "minimum" && n < **tag.dest || tag.name == "maximum" && n > **tag.dest) {
				return fail("invalid_schema", "")
			}
			*tag.dest = &n
		}
	}
	if text, ok := tags.Lookup("order"); ok {
		n, err := strconv.Atoi(text)
		if err != nil {
			return fail("invalid_schema", "")
		}
		s.UI.Order = n
	}
	if text, ok := tags.Lookup("secret"); ok {
		if text != "true" && text != "false" {
			return fail("invalid_schema", "")
		}
		s.WriteOnly = text == "true"
	}
	if text, ok := tags.Lookup("enum"); ok {
		if err := json.Unmarshal([]byte(text), &s.Enum); err != nil || s.Enum == nil {
			return fail("invalid_schema", "")
		}
	}
	if text, ok := tags.Lookup("optionLabels"); ok {
		if err := json.Unmarshal([]byte(text), &s.UI.OptionLabels); err != nil {
			return fail("invalid_schema", "")
		}
	}
	if text, ok := tags.Lookup("default"); ok {
		s.Default = json.RawMessage(text)
	}
	if s.UI.Widget == "" && s.UI.Group == "" && s.UI.Order == 0 && len(s.UI.OptionLabels) == 0 {
		s.UI = nil
	}
	return nil
}
