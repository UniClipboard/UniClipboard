// Package builtinclassify annotates captured and received content by type.
package builtinclassify

import (
	"context"
	"errors"
	"fmt"
	"regexp"
	"strings"
	"unicode/utf8"

	"github.com/uniclipboard/unic/plugin"
)

const ID = "builtin.classify"
const scanLimit = 64 << 10
const MaxRules = 16
const MaxPatternBytes = 1024

var linkPattern = regexp.MustCompile(`(?i)https?://[^\s<>"]+`)

// Rule adds Tag when Pattern matches the text preview.
type Rule struct {
	Pattern string `toml:"pattern" json:"pattern"`
	Tag     string `toml:"tag" json:"tag"`
}

type compiledRule struct {
	pattern *regexp.Regexp
	tag     string
}

type Plugin struct {
	rules []compiledRule
}

func New(rules []Rule) (*Plugin, error) {
	if len(rules) > MaxRules {
		return nil, fmt.Errorf("at most %d classification rules are allowed", MaxRules)
	}
	p := &Plugin{rules: make([]compiledRule, 0, len(rules))}
	for i, rule := range rules {
		if strings.TrimSpace(rule.Tag) == "" || len(rule.Tag) > 64 || !utf8.ValidString(rule.Tag) {
			return nil, fmt.Errorf("classification rule %d: tag must contain 1 to 64 UTF-8 bytes", i+1)
		}
		if len(rule.Pattern) == 0 || len(rule.Pattern) > MaxPatternBytes {
			return nil, fmt.Errorf("classification rule %d: pattern must contain 1 to %d bytes", i+1, MaxPatternBytes)
		}
		pattern, err := regexp.Compile(rule.Pattern)
		if err != nil {
			return nil, fmt.Errorf("classification rule %d: invalid regular expression", i+1)
		}
		p.rules = append(p.rules, compiledRule{pattern: pattern, tag: rule.Tag})
	}
	return p, nil
}

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
func (p *Plugin) Process(ctx context.Context, c plugin.ProcessContext) error {
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
			if err := c.AddTag(plugin.TagLink); err != nil {
				return err
			}
		}
		for _, rule := range p.rules {
			if err := ctx.Err(); err != nil {
				return err
			}
			if rule.pattern.MatchString(text) {
				if err := c.AddTag(rule.tag); err != nil {
					return err
				}
			}
		}
	}
	return nil
}
