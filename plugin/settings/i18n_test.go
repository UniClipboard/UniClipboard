package settings_test

import (
	"encoding/json"
	"reflect"
	"testing"

	"github.com/uniclipboard/unic/plugin/settings"
)

const i18nSchema = `{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "x-unic-settings-version": 1,
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "mode": {
      "type": "string",
      "title": "处理模式",
      "description": "默认说明",
      "enum": ["plain", "markdown"],
      "x-unic-ui": {
        "group": "文本",
        "option_labels": ["纯文本", "Markdown"],
        "i18n": {
          "en": {"title": "Mode", "group": "Text", "option_labels": ["Plain", "Markdown"]},
          "pt-BR": {"title": "Modo"},
          "ja": {"description": "説明"}
        }
      }
    }
  }
}`

func TestLocalizedText(t *testing.T) {
	d, err := settings.Parse([]byte(i18nSchema))
	if err != nil {
		t.Fatal(err)
	}
	var s settings.Schema
	if err := unmarshal(d.JSON(), &s); err != nil {
		t.Fatal(err)
	}
	mode := s.Properties["mode"]
	cases := []struct {
		language string
		want     settings.Text
	}{
		{"", settings.Text{Title: "处理模式", Description: "默认说明", Group: "文本", OptionLabels: []string{"纯文本", "Markdown"}}},
		{"zh-CN", settings.Text{Title: "处理模式", Description: "默认说明", Group: "文本", OptionLabels: []string{"纯文本", "Markdown"}}},
		{"en", settings.Text{Title: "Mode", Description: "默认说明", Group: "Text", OptionLabels: []string{"Plain", "Markdown"}}},
		{"EN-gb", settings.Text{Title: "Mode", Description: "默认说明", Group: "Text", OptionLabels: []string{"Plain", "Markdown"}}},
		{"pt-BR", settings.Text{Title: "Modo", Description: "默认说明", Group: "文本", OptionLabels: []string{"纯文本", "Markdown"}}},
		{"ja", settings.Text{Title: "处理模式", Description: "説明", Group: "文本", OptionLabels: []string{"纯文本", "Markdown"}}},
	}
	for _, c := range cases {
		if got := mode.Localized(c.language); !reflect.DeepEqual(got, c.want) {
			t.Errorf("Localized(%q) = %+v, want %+v", c.language, got, c.want)
		}
	}
}

func TestI18NRejection(t *testing.T) {
	mutate := func(ui string) []byte {
		return []byte(`{"$schema":"https://json-schema.org/draft/2020-12/schema","x-unic-settings-version":1,"type":"object","additionalProperties":false,"properties":{"mode":{"type":"string","enum":["a","b"],"x-unic-ui":` + ui + `}}}`)
	}
	for name, ui := range map[string]string{
		"bad tag":       `{"i18n":{"not a tag":{"title":"x"}}}`,
		"null entry":    `{"i18n":{"en":null}}`,
		"label count":   `{"i18n":{"en":{"option_labels":["only"]}}}`,
		"control text":  `{"i18n":{"en":{"title":"a\u0007b"}}}`,
		"unknown field": `{"i18n":{"en":{"label":"x"}}}`,
	} {
		if _, err := settings.Parse(mutate(ui)); err == nil {
			t.Errorf("%s: schema was accepted", name)
		}
	}
	if _, err := settings.Parse(mutate(`{"i18n":{"en":{"title":"ok","option_labels":["A","B"]}}}`)); err != nil {
		t.Fatal(err)
	}
}

func TestGenerateI18NTag(t *testing.T) {
	type Config struct {
		Mode string `json:"mode" title:"模式" i18n:"{\"en\":{\"title\":\"Mode\"}}"`
	}
	raw, err := settings.Generate[Config]()
	if err != nil {
		t.Fatal(err)
	}
	d, err := settings.Parse(raw)
	if err != nil {
		t.Fatal(err)
	}
	var s settings.Schema
	if err := unmarshal(d.JSON(), &s); err != nil {
		t.Fatal(err)
	}
	if got := s.Properties["mode"].Localized("en").Title; got != "Mode" {
		t.Fatalf("title=%q", got)
	}
}

func unmarshal(data []byte, v any) error { return json.Unmarshal(data, v) }
