package settingsdemo

import (
	"context"
	_ "embed"
	"encoding/json"
	"unicode/utf8"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/settings"
)

//go:embed settings.schema.json
var schemaJSON []byte

// Plugin is a runnable processor example. Its UI schema and strict Init
// validation share the same generated artifact.
type Plugin struct{ config Config }

func (*Plugin) Spec() plugin.Spec {
	return plugin.Spec{
		ID: "example.settingsdemo", Version: "1.0.0", APIVersion: 1,
		Capabilities: []string{plugin.CapProcessor},
	}
}

func (p *Plugin) Init(_ context.Context, _ plugin.Host, cfg plugin.Config) error {
	d, err := settings.Parse(schemaJSON)
	if err != nil {
		return plugin.ErrInvalidConfig
	}
	raw, err := json.Marshal(cfg)
	if err != nil {
		return plugin.ErrInvalidConfig
	}
	// Engine passes a JSON object; the example also allows a nil Config.
	if cfg == nil {
		raw = []byte("{}")
	}
	normalized, err := d.Normalize(raw)
	if err != nil {
		return plugin.ErrInvalidConfig
	}
	var next Config
	if err := json.Unmarshal(normalized, &next); err != nil {
		return plugin.ErrInvalidConfig
	}
	// Business constraints can be stricter than the portable field schema.
	if len(next.Prefix) > 64 {
		return plugin.ErrInvalidConfig
	}
	for _, tag := range next.Tags {
		if tag == "" || len(tag) > 64 {
			return plugin.ErrInvalidConfig
		}
	}
	p.config = next
	return nil
}

func (*Plugin) Start(context.Context) error { return nil }
func (p *Plugin) Close(context.Context) error {
	p.config = Config{}
	return nil
}

func (p *Plugin) Process(ctx context.Context, c plugin.ProcessContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	item := c.Item()
	if !p.config.Enabled || item.Type != "text" || utf8.RuneCountInString(item.Text) > p.config.Limit {
		return nil
	}
	for _, tag := range append([]string{p.config.Prefix}, p.config.Tags...) {
		if err := c.AddTag(tag); err != nil {
			return err
		}
	}
	return nil
}
