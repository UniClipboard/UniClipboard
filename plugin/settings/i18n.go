package settings

import (
	"regexp"
	"strings"
	"unicode"
	"unicode/utf8"
)

// MaxLanguages bounds the per-language display overrides one plugin declares.
const MaxLanguages = 16

var languageTag = regexp.MustCompile(`^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8}){0,3}$`)

// ValidLanguage reports whether tag is an accepted language key for plugin
// display overrides, such as "en", "ja" or "pt-BR".
func ValidLanguage(tag string) bool { return len(tag) <= 35 && languageTag.MatchString(tag) }

// ValidText reports whether s is bounded, valid UTF-8 display text without
// control characters. Empty text is valid and means "not overridden".
func ValidText(s string, maxRunes int) bool {
	return utf8.ValidString(s) && utf8.RuneCountInString(s) <= maxRunes && !strings.ContainsFunc(s, unicode.IsControl)
}

// LanguageChain lists the keys to try for a UI language, most specific first:
// the language itself, then its primary subtag. The default (untagged) text is
// the implicit last step and is not part of the chain.
func LanguageChain(language string) []string {
	if language == "" {
		return nil
	}
	chain := []string{language}
	if i := strings.IndexByte(language, '-'); i > 0 {
		chain = append(chain, language[:i])
	}
	return chain
}

// Pick returns the entry that best matches language: exact tag first, then the
// primary subtag. Matching ignores letter case.
func Pick[T any](values map[string]T, language string) (T, bool) {
	for _, tag := range LanguageChain(language) {
		for key, value := range values {
			if strings.EqualFold(key, tag) {
				return value, true
			}
		}
	}
	var zero T
	return zero, false
}

// UIText is one language's overrides for a field's display text. Empty fields
// keep the default text.
type UIText struct {
	Title        string   `json:"title,omitempty"`
	Description  string   `json:"description,omitempty"`
	Group        string   `json:"group,omitempty"`
	OptionLabels []string `json:"option_labels,omitempty"`
}

// Text is the resolved display text of one schema node.
type Text struct {
	Title        string
	Description  string
	Group        string
	OptionLabels []string
}

// Localized resolves display text for language. Each field falls back
// independently: exact tag, primary subtag, then the default text.
func (s *Schema) Localized(language string) Text {
	out := Text{Title: s.Title, Description: s.Description}
	if s.UI == nil {
		return out
	}
	out.Group, out.OptionLabels = s.UI.Group, s.UI.OptionLabels
	var title, description, group, labels bool
	for _, tag := range LanguageChain(language) {
		var override *UIText
		for key, value := range s.UI.I18N {
			if strings.EqualFold(key, tag) {
				override = value
				break
			}
		}
		if override == nil {
			continue
		}
		if !title && override.Title != "" {
			out.Title, title = override.Title, true
		}
		if !description && override.Description != "" {
			out.Description, description = override.Description, true
		}
		if !group && override.Group != "" {
			out.Group, group = override.Group, true
		}
		if !labels && len(override.OptionLabels) > 0 {
			out.OptionLabels, labels = override.OptionLabels, true
		}
	}
	return out
}

func checkI18N(s *Schema) bool {
	u := s.UI
	if len(u.I18N) > MaxLanguages {
		return false
	}
	for tag, text := range u.I18N {
		if !ValidLanguage(tag) || text == nil ||
			!ValidText(text.Title, 256) || !ValidText(text.Description, 1024) || !ValidText(text.Group, 128) {
			return false
		}
		if len(text.OptionLabels) > 0 && len(text.OptionLabels) != len(s.Enum) {
			return false
		}
		for _, label := range text.OptionLabels {
			if !ValidText(label, 256) {
				return false
			}
		}
	}
	return true
}
