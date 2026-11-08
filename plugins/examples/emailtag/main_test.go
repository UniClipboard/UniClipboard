package main

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugin"
)

type pc struct {
	item plugin.Item
	tags []string
}

func (c *pc) Event() string                 { return plugin.EventCaptured }
func (c *pc) Item() plugin.Item             { return c.item }
func (c *pc) Policy() plugin.Policy         { return plugin.Policy{} }
func (c *pc) AddTag(t string) error         { c.tags = append(c.tags, t); return nil }
func (c *pc) RemoveTag(string) error        { return nil }
func (c *pc) SetPersist(bool) error         { return nil }
func (c *pc) SetSync(bool) error            { return nil }
func (c *pc) SetExpire(time.Duration) error { return nil }

func TestTemplate(t *testing.T) {
	var _ plugin.Processor = (*emailtag)(nil)
	p := &emailtag{}
	if err := p.Init(context.Background(), nil, plugin.Config{"tag": "mail"}); err != nil {
		t.Fatal(err)
	}
	c := &pc{item: plugin.Item{Type: "text", Text: "me@example.com"}}
	if err := p.Process(context.Background(), c); err != nil || len(c.tags) != 1 || c.tags[0] != "mail" {
		t.Fatalf("%v %v", c.tags, err)
	}
	c = &pc{item: plugin.Item{Type: "text", Text: "nothing"}}
	if _ = p.Process(context.Background(), c); len(c.tags) != 0 {
		t.Fatal("tagged plain text")
	}
	for _, cfg := range []plugin.Config{{"x": 1}, {"tag": ""}, {"tag": 3}} {
		if err := (&emailtag{}).Init(context.Background(), nil, cfg); !errors.Is(err, plugin.ErrInvalidConfig) {
			t.Fatalf("%v: %v", cfg, err)
		}
	}
}
