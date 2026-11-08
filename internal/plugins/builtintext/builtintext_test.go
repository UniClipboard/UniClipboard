package builtintext

import (
	"context"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugin"
)

type procCtx struct {
	item plugin.Item
	tags []string
}

func (c *procCtx) Event() string                 { return plugin.EventCaptured }
func (c *procCtx) Item() plugin.Item             { return c.item }
func (c *procCtx) Policy() plugin.Policy         { return plugin.Policy{} }
func (c *procCtx) AddTag(t string) error         { c.tags = append(c.tags, t); return nil }
func (c *procCtx) RemoveTag(string) error        { return nil }
func (c *procCtx) SetPersist(bool) error         { return nil }
func (c *procCtx) SetSync(bool) error            { return nil }
func (c *procCtx) SetExpire(time.Duration) error { return nil }

type actCtx struct {
	id, text, out string
}

func (c *actCtx) Action() string         { return c.id }
func (c *actCtx) Item() plugin.Item      { return plugin.Item{Type: "text", Text: c.text} }
func (c *actCtx) SetText(s string) error { c.out = s; return nil }

func TestTags(t *testing.T) {
	for text, want := range map[string][]string{
		"write to me@example.com":           {"email"},
		"see https://example.com/a?b=1 now": {"url"},
		"me@example.com and http://x.org/":  {"email", "url"},
		"nothing here, at 5 o'clock":        nil,
		"a@b":                               nil,
	} {
		c := &procCtx{item: plugin.Item{Type: "text", Text: text}}
		if err := New().Process(context.Background(), c); err != nil {
			t.Fatal(err)
		}
		if len(c.tags) != len(want) {
			t.Errorf("%q: %v", text, c.tags)
			continue
		}
		for i := range want {
			if c.tags[i] != want[i] {
				t.Errorf("%q: %v", text, c.tags)
			}
		}
	}
	c := &procCtx{item: plugin.Item{Type: "image", Text: "me@example.com"}}
	_ = New().Process(context.Background(), c)
	if len(c.tags) != 0 {
		t.Fatal("tagged a non-text item")
	}
}

func TestActions(t *testing.T) {
	p := New()
	for id, want := range map[string]string{"upper": "  ABC ", "lower": "  abc ", "trim": "AbC", "plain": "  AbC "} {
		c := &actCtx{id: id, text: "  AbC "}
		if err := p.Run(context.Background(), c); err != nil {
			t.Fatal(id, err)
		}
		if c.out != want {
			t.Errorf("%s: %q", id, c.out)
		}
	}
	if err := p.Run(context.Background(), &actCtx{id: "nope"}); err == nil {
		t.Fatal("unknown action accepted")
	}
	if err := p.Init(context.Background(), nil, plugin.Config{"x": 1}); err == nil {
		t.Fatal("unknown setting accepted")
	}
}
