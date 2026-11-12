// Package builtinclassify annotates captured and received content by type.
package builtinclassify

import (
	"context"
	"errors"
	"regexp"

	"github.com/uniclipboard/unic/plugin"
)

const ID = "builtin.classify"
const scanLimit = 64 << 10

var linkPattern = regexp.MustCompile(`(?i)https?://[^\s<>"]+`)

type Plugin struct{}

func New() *Plugin { return &Plugin{} }
func (*Plugin) Spec() plugin.Spec {
	return plugin.Spec{ID: ID, Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}}
}
func (*Plugin) Init(_ context.Context, _ plugin.Host, cfg plugin.Config) error {
	if len(cfg) != 0 {
		return errors.New("builtin.classify takes no settings")
	}
	return nil
}
func (*Plugin) Start(context.Context) error { return nil }
func (*Plugin) Close(context.Context) error { return nil }
func (*Plugin) Process(_ context.Context, c plugin.ProcessContext) error {
	item := c.Item()
	var tag string
	switch item.Type {
	case "text":
		tag = plugin.TagText
	case "image":
		tag = plugin.TagImage
	case "files":
		tag = plugin.TagFiles
	case "directory", "directory_reference":
		tag = plugin.TagFolder
	default:
		return nil
	}
	if err := c.AddTag(tag); err != nil {
		return err
	}
	if item.Type == "text" {
		text := item.Text
		if len(text) > scanLimit {
			text = text[:scanLimit]
		}
		if linkPattern.MatchString(text) {
			return c.AddTag(plugin.TagLink)
		}
	}
	return nil
}
