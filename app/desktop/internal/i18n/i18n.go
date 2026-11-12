// Package i18n resolves desktop-owned UI text without UI or service dependencies.
package i18n

import (
	_ "embed"
	"encoding/json"
)

// Chinese source strings are catalogue IDs. User content never passes through
// this lookup. The empty language preserves the existing Chinese default.
//
//go:embed locales.json
var catalogue []byte

var translations = func() map[string]map[string]string {
	var values map[string]map[string]string
	if err := json.Unmarshal(catalogue, &values); err != nil {
		panic(err)
	}
	return values
}()

// Translate resolves application-owned UI text for the desktop host.
func Translate(language, source string) string {
	if text := translations[source][language]; text != "" {
		return text
	}
	return source
}
