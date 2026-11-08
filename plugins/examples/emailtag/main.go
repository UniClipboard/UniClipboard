// Command emailtag is the minimal Go plugin template: a processor that tags
// text containing an email address. Build it as its own program; it does not
// require rebuilding unicd.
//
//	go build -o emailtag ./plugins/examples/emailtag
package main

import (
	"context"
	"log"
	"regexp"

	"github.com/uniclipboard/unic/plugin"
)

type emailtag struct{ tag string }

func main() {
	if err := plugin.Serve(&emailtag{}); err != nil {
		log.Fatal(err) // never log clipboard content
	}
}

func (*emailtag) Spec() plugin.Spec {
	return plugin.Spec{ID: "example.emailtag", Version: "1.0.0", APIVersion: 1,
		Capabilities: []string{plugin.CapProcessor}}
}

func (p *emailtag) Init(_ context.Context, _ plugin.Host, cfg plugin.Config) error {
	for key := range cfg {
		if key != "tag" {
			return plugin.ErrInvalidConfig
		}
	}
	p.tag = "email"
	if v, ok := cfg["tag"]; ok {
		s, valid := v.(string)
		if !valid || s == "" {
			return plugin.ErrInvalidConfig
		}
		p.tag = s
	}
	return nil
}

func (*emailtag) Start(context.Context) error { return nil }
func (*emailtag) Close(context.Context) error { return nil }

var email = regexp.MustCompile(`[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}`)

func (p *emailtag) Process(ctx context.Context, c plugin.ProcessContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if item := c.Item(); item.Type == "text" && email.MatchString(item.Text) {
		return c.AddTag(p.tag) // annotate only; the content is stored and synced unchanged
	}
	return nil
}
