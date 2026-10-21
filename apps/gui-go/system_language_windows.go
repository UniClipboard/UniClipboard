package main

import "golang.org/x/sys/windows"

// systemLanguage is the display language the system prefers, as the quick panel reads it
// (apps/quick-panel/src/platform/win32/locale.rs): the first preferred UI language.
func systemLanguage() string {
	languages, err := windows.GetUserPreferredUILanguages(windows.MUI_LANGUAGE_NAME)
	if err != nil || len(languages) == 0 {
		return ""
	}
	return languages[0]
}
