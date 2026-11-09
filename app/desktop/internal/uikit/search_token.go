package uikit

import "github.com/egoist/mygo/ui"

// SearchFieldToken mirrors contract.ts SearchFieldToken: a condition shown as a token inside
// a TokenSearchField, e.g. "Tag #deploy".
type SearchFieldToken struct {
	ID          string
	Field       string // the condition's field, already localized; display only
	Value       string
	Kind        SearchTokenKind
	RemoveLabel string // accessible name of the remove button, e.g. "Remove Tag #deploy"
}

// TokenSearchFieldProps mirrors contract.ts TokenSearchFieldProps. The caller owns the search
// model: text, tokens, suggestions and which one is highlighted; OnKey sees every key first.
type TokenSearchFieldProps struct {
	Label        string
	Placeholder  string
	ShortcutHint string // display-only, e.g. "⌘F"
	Value        *string
	// Feedback is immediate completion or literal-search feedback shown above the suggestions.
	Feedback    string
	Tokens      []SearchFieldToken
	Suggestions []SearchSuggestion
	// Open is optional two-way state of the suggestion menu: it is set when the field gains
	// focus or its text changes, cleared when it loses focus, and the menu shows while it is
	// true and the field has focus. Nil shows the menu whenever the field has focus.
	Open *bool
	// Highlighted is the highlighted suggestion (-1 for none). The caller moves it with the
	// arrow keys (via OnKey); pointer hover writes it.
	Highlighted *int
	// OnKey sees each key before the field acts on it (the contract's onKeyDown); returning
	// true prevents the default action. Keys during input-method composition are never
	// delivered.
	OnKey func(SearchFieldKey) bool
	// ClearLabel names the trailing clear-all button, shown while there is text or a token.
	ClearLabel        string
	SuggestionsFooter string
	Disabled          bool
}

// TokenSearchResult is what TokenSearchField reports for one frame.
type TokenSearchResult struct {
	Element, Input ui.Element
	Edited         bool   // the text changed
	Submitted      bool   // Return reached the field (OnKey did not prevent it)
	Selected       int    // index of the suggestion clicked in this frame, -1 for none
	RemovedToken   string // ID of the token whose remove button was pressed, "" for none
	Cleared        bool   // the clear-all button was pressed
}

// TokenSearchField is a search field with condition tokens and a suggestion menu (macOS
// search tokens, WinUI AutoSuggestBox with tokens). The input has the combobox role and the
// menu a listbox with options, the highlighted one selected.
func (k Kit) TokenSearchField(c *ui.Context, p TokenSearchFieldProps) TokenSearchResult {
	pal := k.fldPal()
	switch k.Skin {
	case MacOS:
		pal.h, pal.radius, pal.size, pal.padX, pal.bg = 32, 16, 13, 10, pal.fill
	case Windows:
		pal.padX = 6
	}
	local := -1
	hi := p.Highlighted
	if hi == nil {
		hi = &local
	}
	res := TokenSearchResult{Selected: -1}
	hasText := p.Value != nil && *p.Value != ""
	active := hasText || len(p.Tokens) > 0
	var in ui.Element
	row := ui.Row(c).AlignItems(ui.Center).Gap(6).PaddingX(pal.padX).MinWidth(220)
	row.Children(func() {
		glyph := func() { k.Icon(c.Key("glyph"), IconSearch, 16).TextColor(pal.fg2).Shrink(0) }
		if k.Skin != Windows {
			glyph()
		}
		for _, t := range p.Tokens {
			r := k.SearchToken(c.Key("token:"+t.ID), SearchTokenProps{Field: t.Field, Value: t.Value, Kind: t.Kind, RemoveLabel: t.RemoveLabel})
			if r.Removed {
				res.RemovedToken = t.ID
			}
		}
		placeholder := p.Placeholder
		if len(p.Tokens) > 0 {
			placeholder = ""
		}
		in = fldBareInput(ui.TextInputBase(c, p.Value).Label(p.Label).Disabled(p.Disabled).Role(ui.RoleComboBox), pal, placeholder)
		in.MinWidth(64)
		if p.OnKey != nil {
			on := p.OnKey
			in.OnTextKey(func(tk ui.TextKey) bool { return on(SearchFieldKey{Key: tk.Key, Mods: tk.Mods}) })
		}
		if !active && p.ShortcutHint != "" {
			ui.Text(c.Key("hint"), p.ShortcutHint).FontSize(pal.size - 2).TextColor(pal.hint)
		}
		if active && p.ClearLabel != "" {
			clear := ui.ButtonBase(c.Key("clear")).Label(p.ClearLabel).Size(20, 20).Radius(10).Shrink(0).Center().
				Background(pal.fill).TextColor(pal.fg).KeepFocus()
			clear.Children(func() { k.Icon(c, IconDismiss, 10) })
			if clear.Clicked() {
				res.Cleared = true
			}
		}
		if k.Skin == Windows {
			glyph()
		}
	})
	focused := in.Focused()
	k.fldStyle(row, pal, focused, false, p.Disabled)
	row.Role(ui.RoleSearch)

	// Menu visibility. Typing reopens a closed menu; losing focus closes it.
	if p.Open != nil {
		gained := ui.Local(in, "uikit.focus", func() bool { return false })
		if focused && !*gained || in.Changed() {
			*p.Open = true
		}
		if !focused {
			*p.Open = false
		}
		*gained = focused
	}
	open := focused && (p.Open == nil || *p.Open) && (len(p.Suggestions) > 0 || p.Feedback != "")
	ui.PopoverBase(c, row, &open, func(panel ui.Element) {
		k.fldPopup(panel, pal, max(row.Bounds().W, 280))
		res.Selected = k.fldSuggestions(c, p.Suggestions, suggestCfg{
			label: p.Label, footer: p.SuggestionsFooter, feedback: p.Feedback, hi: hi,
		})
	})
	res.Element, res.Input = row, in
	res.Edited, res.Submitted = in.Changed(), in.Submitted()
	return res
}
