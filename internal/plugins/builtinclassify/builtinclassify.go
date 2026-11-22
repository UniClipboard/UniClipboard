// Package builtinclassify annotates captured and received content by type.
package builtinclassify

import (
	"context"
	"errors"
	"fmt"
	"regexp"
	"slices"
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

var defaultRules = [...]compiledRule{
	{tag: plugin.TagColor, pattern: regexp.MustCompile(`(?i)^#[0-9a-f]{6}([0-9a-f]{2})?$`)},
	{tag: plugin.TagVersion, pattern: regexp.MustCompile(`^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$`)},
	{tag: plugin.TagUUID, pattern: regexp.MustCompile(`(?i)^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$`)},
	{tag: plugin.TagIPv4, pattern: regexp.MustCompile(`^((25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])\.){3}(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])$`)},
}

// DefaultTags returns the tag IDs of the individually switchable rules.
func DefaultTags() []string {
	tags := make([]string, 0, len(defaultRules))
	for _, rule := range defaultRules {
		tags = append(tags, rule.tag)
	}
	return tags
}

type Plugin struct {
	defaults []compiledRule
	rules    []compiledRule
}

func New(rules []Rule, disabledDefaultTags []string) (*Plugin, error) {
	if len(rules) > MaxRules {
		return nil, fmt.Errorf("at most %d classification rules are allowed", MaxRules)
	}
	p := &Plugin{rules: make([]compiledRule, 0, len(rules))}
	defaultTags := DefaultTags()
	for i, tag := range disabledDefaultTags {
		if !slices.Contains(defaultTags, tag) {
			return nil, fmt.Errorf("unknown default classification tag at position %d", i+1)
		}
		if slices.Contains(disabledDefaultTags[:i], tag) {
			return nil, fmt.Errorf("duplicate default classification tag at position %d", i+1)
		}
	}
	for _, rule := range defaultRules {
		if !slices.Contains(disabledDefaultTags, rule.tag) {
			p.defaults = append(p.defaults, rule)
		}
	}
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
	if item.Type != "text" {
		return nil
	}
	text := item.Text
	if len(text) > scanLimit {
		text = text[:scanLimit]
	} else if err := addMatchingTags(ctx, c, strings.TrimSpace(text), p.defaults); err != nil {
		return err
	}
	if linkPattern.MatchString(text) {
		if err := c.AddTag(plugin.TagLink); err != nil {
			return err
		}
	}
	return addMatchingTags(ctx, c, text, p.rules)
}

func addMatchingTags(ctx context.Context, c plugin.ProcessContext, text string, rules []compiledRule) error {
	for _, rule := range rules {
		if err := ctx.Err(); err != nil {
			return err
		}
		if rule.pattern.MatchString(text) {
			if err := c.AddTag(rule.tag); err != nil {
				return err
			}
		}
	}
	return nil
}
