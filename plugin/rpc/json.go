package rpc

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"strconv"
	"unicode/utf8"
)

// MaxJSONDepth is the deepest object/array nesting a payload may have.
const MaxJSONDepth = 32

// Decode parses one JSON object into v. It rejects invalid UTF-8, duplicate
// keys, unknown fields, trailing content and nesting deeper than MaxJSONDepth.
func Decode(data []byte, v any) error {
	if !utf8.Valid(data) {
		return protoErr("payload is not valid UTF-8")
	}
	trimmed := bytes.TrimLeft(data, " \t\r\n")
	if len(trimmed) == 0 || trimmed[0] != '{' {
		return protoErr("payload must be a JSON object")
	}
	dec := json.NewDecoder(bytes.NewReader(data))
	if err := scanValue(dec, 0); err != nil {
		return err
	}
	if _, err := dec.Token(); !errors.Is(err, io.EOF) {
		return protoErr("trailing content")
	}
	dec = json.NewDecoder(bytes.NewReader(data))
	dec.DisallowUnknownFields()
	if err := dec.Decode(v); err != nil {
		return protoErr("invalid payload")
	}
	return nil
}

func scanValue(dec *json.Decoder, depth int) error {
	tok, err := dec.Token()
	if err != nil {
		return protoErr("invalid JSON")
	}
	d, ok := tok.(json.Delim)
	if !ok {
		return nil
	}
	if depth+1 > MaxJSONDepth {
		return protoErr("JSON nested too deeply")
	}
	if d == '{' {
		keys := map[string]struct{}{}
		for dec.More() {
			kt, err := dec.Token()
			if err != nil {
				return protoErr("invalid JSON")
			}
			k, _ := kt.(string)
			if _, dup := keys[k]; dup {
				return protoErr("duplicate key")
			}
			keys[k] = struct{}{}
			if err := scanValue(dec, depth+1); err != nil {
				return err
			}
		}
	} else {
		for dec.More() {
			if err := scanValue(dec, depth+1); err != nil {
				return err
			}
		}
	}
	if _, err := dec.Token(); err != nil {
		return protoErr("invalid JSON")
	}
	return nil
}

// Encode marshals v as a payload.
func Encode(v any) ([]byte, error) { return json.Marshal(v) }

// U64 is a uint64 carried as a canonical decimal string (0, or no leading
// zero) so no precision is lost in languages with float-only numbers.
type U64 uint64

func (u U64) MarshalJSON() ([]byte, error) {
	return strconv.AppendQuote(nil, strconv.FormatUint(uint64(u), 10)), nil
}

func (u *U64) UnmarshalJSON(b []byte) error {
	s, err := strconv.Unquote(string(b))
	if err != nil || len(b) < 3 || len(s) > 20 || (len(s) > 1 && s[0] == '0') {
		return protoErr("invalid uint64 string")
	}
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return protoErr("invalid uint64 string")
		}
	}
	n, err := strconv.ParseUint(s, 10, 64)
	if err != nil {
		return protoErr("uint64 out of range")
	}
	*u = U64(n)
	return nil
}
