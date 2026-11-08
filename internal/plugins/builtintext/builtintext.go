// Package builtintext is the plugin that ships inside the unic binary. It is
// an ordinary plugin: it implements plugin.Processor and plugin.Actor and gets
// no powers the others would not. It tags text that holds an email address or
// a link, and offers a few text actions.
package builtintext

import (
	"context"
	"errors"
	"regexp"
	"strings"

	"github.com/uniclipboard/unic/plugin"
)

// ID is the plugin's identifier, also used as the key of its settings.
const ID = "builtin.text"

// scanLimit bounds how much text is looked at; tags are hints, not guarantees.
const scanLimit = 64 << 10

var (
	emailRe = regexp.MustCompile(`[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}`)
	urlRe   = regexp.MustCompile(`(?i)https?://[^\s<>"]+`)
)

type Plugin struct{}

func New() *Plugin { return &Plugin{} }

func (*Plugin) Spec() plugin.Spec {
	return plugin.Spec{
		ID: ID, Version: "1.0.0", APIVersion: 1,
		Capabilities: []string{plugin.CapProcessor, plugin.CapAction},
		Actions: []plugin.ActionSpec{
			{ID: "upper", Title: "转大写", Accepts: []string{"text"}},
			{ID: "lower", Title: "转小写", Accepts: []string{"text"}},
			{ID: "trim", Title: "去除首尾空白", Accepts: []string{"text"}},
			{ID: "plain", Title: "纯文本", Accepts: []string{"text"}},
		},
	}
}

func (*Plugin) Init(_ context.Context, _ plugin.Host, cfg plugin.Config) error {
	if len(cfg) != 0 {
		return errors.New("builtin.text takes no settings")
	}
	return nil
}
func (*Plugin) Start(context.Context) error { return nil }
func (*Plugin) Close(context.Context) error { return nil }

// Process tags text that contains an email address ("email") or a web link ("url").
func (*Plugin) Process(_ context.Context, c plugin.ProcessContext) error {
	it := c.Item()
	if it.Type != "text" || it.Text == "" {
		return nil
	}
	text := it.Text
	if len(text) > scanLimit {
		text = text[:scanLimit]
	}
	if emailRe.MatchString(text) {
		if err := c.AddTag("email"); err != nil {
			return err
		}
	}
	if urlRe.MatchString(text) {
		return c.AddTag("url")
	}
	return nil
}

func (*Plugin) Run(_ context.Context, c plugin.ActionContext) error {
	text := c.Item().Text
	switch c.Action() {
	case "upper":
		text = strings.ToUpper(text)
	case "lower":
		text = strings.ToLower(text)
	case "trim":
		text = strings.TrimSpace(text)
	case "plain": // the result is text only, so rich formats are dropped
	default:
		return errors.New("unknown action")
	}
	return c.SetText(text)
}
