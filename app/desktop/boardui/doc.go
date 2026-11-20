// Package boardui provides native mygo controls derived from the MIT BoardUI
// source export ec5c5eb3e29c2441be63b9422cd54f411ef23466.
//
// Hosts call LoadFonts before creating views and Kit.Apply at the start of a
// view. Values and open flags belong to the caller. Stable c.Key identities
// preserve mygo editor, focus and overlay state. Controls return their actual
// interactive element; callers read Clicked, Changed and Submitted from it.
// The package does not own windows, clipboard, network or persisted settings.
//
// Dialog uses mygo modal focus scopes. Select uses a non-modal in-window native mygo
// listbox built from PopoverBase, FocusGroup and ButtonBase on every OS.
// The first slice supports arrows, Home/End, Enter/Space, Escape and Tab;
// typeahead remains outside its supported interface.
// These controls have software-view acceptance; platform-native accessibility
// and IME acceptance require separate evidence.
package boardui
