package settings_test

import (
	"bytes"
	"encoding/json"
	"errors"
	"os"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/uniclipboard/unic/plugin/settings"
	"github.com/uniclipboard/unic/plugins/examples/settingsdemo"
)

const resources = "../../app/mac/Sources/SettingsSchema/Resources/"

func demo(t *testing.T) *settings.Definition {
	t.Helper()
	raw, err := os.ReadFile(resources + "settings-demo.schema.json")
	if err != nil {
		t.Fatal(err)
	}
	d, err := settings.Parse(raw)
	if err != nil {
		t.Fatal(err)
	}
	generated, err := settings.Generate[settingsdemo.Config]()
	if err != nil || !bytes.Equal(generated, bytes.TrimSpace(raw)) {
		t.Fatalf("fixture is stale; run go generate ./plugins/examples/settingsdemo: %v", err)
	}
	packaged, err := os.ReadFile("../../plugins/examples/settingsdemo/settings.schema.json")
	if err != nil || !bytes.Equal(bytes.TrimSpace(packaged), generated) {
		t.Fatalf("packaged schema is stale: %v", err)
	}
	return d
}

func assertCode(t *testing.T, err error, code, path string) {
	t.Helper()
	var e *settings.Error
	if !errors.As(err, &e) || e.Code != code || e.Path != path {
		t.Fatalf("error=%v, want %q at %q", err, code, path)
	}
}

// The Swift tests consume exactly this fixture and the Go-generated schema.
func TestSharedVectors(t *testing.T) {
	d := demo(t)
	raw, err := os.ReadFile(resources + "validation-cases.json")
	if err != nil {
		t.Fatal(err)
	}
	var vectors []struct {
		Name   string          `json:"name"`
		Input  json.RawMessage `json:"input"`
		Output json.RawMessage `json:"output"`
		Code   string          `json:"code"`
		Path   string          `json:"path"`
	}
	if err := json.Unmarshal(raw, &vectors); err != nil {
		t.Fatal(err)
	}
	for _, v := range vectors {
		t.Run(v.Name, func(t *testing.T) {
			out, err := d.Normalize(v.Input)
			if v.Code != "" {
				assertCode(t, err, v.Code, v.Path)
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			var got, want any
			if json.Unmarshal(out, &got) != nil || json.Unmarshal(v.Output, &want) != nil || !reflect.DeepEqual(got, want) {
				t.Fatalf("got %s; want %s", out, v.Output)
			}
			if err := d.Validate(out); err != nil {
				t.Fatal(err)
			}
			var typed settingsdemo.Config
			if err := json.Unmarshal(out, &typed); err != nil {
				t.Fatalf("normalized config cannot decode into source type: %v", err)
			}
		})
	}
}

func TestStrictConfigAndPrivacy(t *testing.T) {
	d := demo(t)
	for _, raw := range []string{
		`{"prefix":"a","prefix":"b"}`, `{} {}`, `{"retry":{"count":1,"count":2}}`,
		`{"notes":NaN}`, `{"notes":` + strings.Repeat("[", 33) + `0` + strings.Repeat("]", 33) + `}`,
		`{"max_length":9007199254740993}`, `{"max_length":1.00000000000000000001}`,
		`{"notes":"` + strings.Repeat("a", settings.MaxConfigBytes) + `"}`,
		string([]byte{'{', '"', 'x', '"', ':', '"', 0xff, '"', '}'}),
	} {
		if _, err := d.Normalize([]byte(raw)); err == nil {
			t.Fatalf("accepted malformed or oversized config")
		}
	}
	err := d.Validate([]byte(`{"user-secret":"value-secret"}`))
	assertCode(t, err, "unknown_property", "")
	if strings.Contains(err.Error(), "secret") {
		t.Fatal("error leaked submitted key/value")
	}
	assertCode(t, d.Validate([]byte(`{}`)), "required", "/enabled")
}

func TestSchemaRejection(t *testing.T) {
	base := `{"$schema":"` + settings.Dialect + `","x-unic-settings-version":1,"type":"object","additionalProperties":false,"properties":{"x":%s}}`
	for _, field := range []string{
		`{"type":"string","pattern":"x"}`,
		`{"type":"string","minimum":null}`,
		`{"type":"string","enum":null}`,
		`{"type":"string","x-unic-ui":{"widget":"inputnumber"}}`,
		`{"type":"string","writeOnly":true,"default":"SECRET"}`,
		`{"type":"string","writeOnly":true,"x-unic-ui":{"widget":"input"}}`,
		`{"type":"integer","enum":[1,1.0]}`,
		`{"type":"string","enum":["a"],"x-unic-ui":{"option_labels":["A","B"]}}`,
		`{"type":"string","enum":["a"],"default":"b"}`,
		`{"type":"string","minLength":2,"maxLength":1}`,
		`{"type":"object","additionalProperties":true}`,
		`{"type":"array","items":{"type":"object","additionalProperties":false}}`,
		`{"type":"integer","minimum":9007199254740992}`,
		`{"type":"string","type":"boolean"}`,
		`{"type":"object","additionalProperties":false,"properties":{"token":{"type":"string","writeOnly":true}},"default":{"token":"SECRET"}}`,
	} {
		raw := strings.Replace(base, "%s", field, 1)
		if _, err := settings.Parse([]byte(raw)); err == nil {
			t.Fatalf("accepted invalid schema: %s", field)
		}
	}
	raw := []byte(strings.Replace(base, "%s", `{"type":"string"}`, 1))
	for _, broken := range [][]byte{
		bytes.Replace(raw, []byte(`"x-unic-settings-version":1`), []byte(`"x-unic-settings-version":2`), 1),
		append(raw, raw...),
		[]byte(strings.Repeat(" ", settings.MaxSchemaBytes+1)),
	} {
		if _, err := settings.Parse(broken); err == nil {
			t.Fatal("accepted unsupported or invalid schema")
		}
	}
}

func TestGeneratorRejectsAmbiguousTypes(t *testing.T) {
	check := func(err error) {
		t.Helper()
		if err == nil {
			t.Fatal("accepted unsupported Go wire shape")
		}
	}
	_, err := settings.Generate[struct{ Field string }]()
	check(err)
	_, err = settings.Generate[struct {
		Field *int `json:"field"`
	}]()
	check(err)
	_, err = settings.Generate[struct {
		Field []byte `json:"field"`
	}]()
	check(err)
	_, err = settings.Generate[struct {
		Field map[string]string `json:"field"`
	}]()
	check(err)
	_, err = settings.Generate[struct {
		Field int8 `json:"field" maximum:"128"`
	}]()
	check(err)
	_, err = settings.Generate[struct {
		Field string `json:"field" required:"yes"`
	}]()
	check(err)
}

func TestDefaultsCopyAndConcurrency(t *testing.T) {
	d := demo(t)
	original := d.JSON()
	corrupt := d.JSON()
	corrupt[0] = '!'
	if !bytes.Equal(d.JSON(), original) {
		t.Fatal("schema data aliases internal state")
	}
	var wg sync.WaitGroup
	for range 12 {
		wg.Go(func() {
			for range 20 {
				out, err := d.Normalize([]byte(`{}`))
				if err != nil || bytes.Contains(out, []byte("token")) {
					t.Errorf("default/secret failure: %v", err)
				}
				if len(out) > 0 {
					out[0] = '!'
				}
			}
		})
	}
	wg.Wait()
}

func TestOptionalObjectAndTypedEnum(t *testing.T) {
	raw := []byte(`{"$schema":"` + settings.Dialect + `","x-unic-settings-version":1,
		"type":"object","additionalProperties":false,"properties":{
			"optional":{"type":"object","additionalProperties":false,
				"properties":{"enabled":{"type":"boolean","default":false}}},
			"choice":{"type":"integer","enum":[1,2],"default":1},
			"switch":{"type":"boolean","enum":[true,false],"default":false}
		}}`)
	d, err := settings.Parse(raw)
	if err != nil {
		t.Fatal(err)
	}
	out, err := d.Normalize([]byte(`{}`))
	if err != nil || bytes.Contains(out, []byte("optional")) {
		t.Fatalf("invented absent object: %s (%v)", out, err)
	}
	out, err = d.Normalize([]byte(`{"optional":{}}`))
	if err != nil || !bytes.Contains(out, []byte(`"optional":{"enabled":false}`)) {
		t.Fatalf("did not fill present object: %s (%v)", out, err)
	}
	_, err = d.Normalize([]byte(`{"choice":"1"}`))
	assertCode(t, err, "type", "/choice")
}
