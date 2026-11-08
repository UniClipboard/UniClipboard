package settings

import (
	"encoding/json"
	"math"
	"strconv"
	"strings"
	"unicode/utf8"
)

func pointer(path, key string) string {
	return path + "/" + strings.ReplaceAll(strings.ReplaceAll(key, "~", "~0"), "/", "~1")
}

func validate(s *Schema, v any, path string) error {
	switch s.Type {
	case "object":
		m, ok := v.(map[string]any)
		if !ok {
			return fail("type", path)
		}
		for k := range m {
			if _, ok := s.Properties[k]; !ok {
				return fail("unknown_property", path)
			}
		}
		// Stable field order gives every shell the same first error.
		required := map[string]bool{}
		for _, k := range s.Required {
			required[k] = true
		}
		for _, k := range keys(s) {
			child, exists := m[k]
			if !exists {
				if required[k] {
					return fail("required", pointer(path, k))
				}
				continue
			}
			if err := validate(s.Properties[k], child, pointer(path, k)); err != nil {
				return err
			}
		}
	case "array":
		a, ok := v.([]any)
		if !ok {
			return fail("type", path)
		}
		if s.MinItems != nil && len(a) < *s.MinItems || s.MaxItems != nil && len(a) > *s.MaxItems {
			return fail("items", path)
		}
		for i, child := range a {
			if err := validate(s.Items, child, pointer(path, strconv.Itoa(i))); err != nil {
				return err
			}
		}
	case "string":
		str, ok := v.(string)
		if !ok {
			return fail("type", path)
		}
		n := utf8.RuneCountInString(str)
		if s.MinLength != nil && n < *s.MinLength || s.MaxLength != nil && n > *s.MaxLength {
			return fail("length", path)
		}
	case "boolean":
		if _, ok := v.(bool); !ok {
			return fail("type", path)
		}
	case "integer", "number":
		n, ok := v.(json.Number)
		if !ok {
			return fail("type", path)
		}
		f, err := n.Float64()
		if err != nil || math.IsNaN(f) || math.IsInf(f, 0) || math.Abs(f) > MaxSafeNumber {
			return fail("range", path)
		}
		if s.Type == "integer" && math.Trunc(f) != f {
			return fail("type", path)
		}
		if s.Minimum != nil && f < *s.Minimum || s.Maximum != nil && f > *s.Maximum {
			return fail("range", path)
		}
	}
	if s.Enum != nil {
		match := false
		for _, raw := range s.Enum {
			value, _ := decodeJSON(raw, MaxConfigBytes)
			if enumKey(value) == enumKey(v) {
				match = true
				break
			}
		}
		if !match {
			return fail("enum", path)
		}
	}
	return nil
}

func defaults(s *Schema, v any) any {
	if n, ok := v.(json.Number); ok && s.Type == "integer" {
		if f, err := n.Float64(); err == nil && !math.IsInf(f, 0) && !math.IsNaN(f) && math.Abs(f) <= MaxSafeNumber && math.Trunc(f) == f {
			// encoding/json cannot decode 1e3 into an int. Normalize its JSON
			// spelling as well as its value before a typed plugin decodes it.
			return json.Number(strconv.FormatFloat(f, 'f', 0, 64))
		}
	}
	if m, ok := v.(map[string]any); ok && s.Type == "object" {
		for _, k := range keys(s) {
			child := s.Properties[k]
			val, exists := m[k]
			if !exists && len(child.Default) > 0 {
				val, _ = decodeJSON(child.Default, MaxConfigBytes)
				exists = true
			}
			if exists {
				m[k] = defaults(child, val)
			}
		}
	}
	if a, ok := v.([]any); ok && s.Type == "array" {
		for i, val := range a {
			a[i] = defaults(s.Items, val)
		}
	}
	return v
}
