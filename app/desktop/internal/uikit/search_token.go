package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

type searchDisclosureState struct{ Open, Request, WasFocused, ReturnFocus bool }

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
	// Toolbar uses the macOS search-icon disclosure without replacing the toolbar.
	Toolbar      bool
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
	// OnSelectionChange uses rune offsets, matching mygo's text editor.
	OnSelectionChange func(start, end int)
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
	var in, trigger ui.Element
	row := ui.Row(c).AlignItems(ui.Center).Gap(6).PaddingX(pal.padX).MinWidth(220)
	disclosure := p.Toolbar && k.Skin == MacOS
	var state *searchDisclosureState
	width := float32(460)
	if disclosure {
		state = ui.Local(row, "disclosure", func() searchDisclosureState { return searchDisclosureState{Open: active} })
		if active {
			state.Open = true
		}
		if !p.Disabled {
			c.OnWindowShortcut(ui.Cmd, ui.KeyF, func() { state.Open, state.Request = true, true })
		}
		target := float32(32)
		if state.Open {
			target = 460
		}
		width = row.AnimateTo("width", target, ui.Animation{Duration: 280 * time.Millisecond, Ease: ui.CubicBezier(.2, 0, 0, 1)})
		row.Padding(0).Gap(0).Height(32).Width(width).MinWidth(width).MaxWidth(width).Shrink(0).Radius(16).Clip()
	} else if p.Toolbar {
		row.Grow(1).MinWidth(80)
	}
	body := func() {
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
		disabled := p.Disabled || (disclosure && !state.Open)
		in = fldBareInput(ui.TextInputBase(c.Key("input"), p.Value).Label(p.Label).Disabled(disabled).Role(ui.RoleComboBox), pal, placeholder)
		in.MinWidth(64)
		if p.OnKey != nil || disclosure {
			start, end := in.TextSelection()
			in.OnTextKey(func(tk ui.TextKey) bool {
				before := tk.Text()
				if p.Value != nil {
					*p.Value = before
				}
				handled := false
				if p.OnKey != nil {
					handled = p.OnKey(SearchFieldKey{Key: tk.Key, Mods: tk.Mods, Text: before, Start: start, End: end})
				}
				if !handled && disclosure && tk.Key == ui.KeyEscape {
					state.Open, state.Request = false, false
					state.ReturnFocus = true
					handled = true
				}
				if handled && p.Value != nil && *p.Value != before {
					tk.SetText(*p.Value)
				}
				return handled
			})
		}
		if disclosure && state.Request && !p.Disabled {
			in.Focus()
			if in.Focused() {
				state.Request = false
			}
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
	}
	row.Children(func() {
		glyph := func() { k.Icon(c.Key("glyph"), IconSearch, 16).TextColor(pal.fg2).Shrink(0) }
		if disclosure {
			trigger = ui.ButtonBase(c.Key("search-icon")).Label(p.Label).Size(32, 32).Shrink(0).KeepFocus().Disabled(p.Disabled).Center().TextColor(pal.fg2)
			trigger.Children(glyph)
			if trigger.Clicked() {
				state.Open, state.Request = true, true
			}
			ui.Row(c.Key("reveal")).Width(max(0, width-32)).Height(32).MinWidth(0).Shrink(0).Clip().AccessHidden(!state.Open).Children(func() {
				ui.Row(c.Key("input-body")).Width(428).MinWidth(428).Shrink(0).Gap(6).PaddingEnd(10).AlignItems(ui.Center).Children(body)
			})
		} else {
			if k.Skin != Windows {
				glyph()
			}
			body()
			if k.Skin == Windows {
				glyph()
			}
		}
	})
	if disclosure && state.ReturnFocus {
		trigger.Focus()
		state.ReturnFocus = false
	}
	focused := in.Focused()
	k.fldStyle(row, pal, focused, false, p.Disabled)
	if k.Skin == MacOS {
		row.Background(ui.Transparent).Border(0, ui.Transparent).NoShadow()
		k.glassSurface(row)
		if focused {
			row.Shadow(0, 0, 0, 3.5, pal.focus)
		}
	}
	row.Role(ui.RoleSearch)
	if disclosure {
		if state.WasFocused && !focused && !state.Request && !active {
			state.Open = false
		}
		state.WasFocused = focused
	}
	if focused && p.OnSelectionChange != nil {
		a, b := in.TextSelection()
		p.OnSelectionChange(a, b)
	}

	// Menu visibility. Typing reopens a closed menu; losing focus closes it.
	if p.Open != nil {
		gained := ui.Local(in, "uikit.focus", func() bool { return false })
		if focused && !*gained || (in.Changed() && p.OnKey == nil) {
			*p.Open = true
		}
		if !focused {
			*p.Open = false
		}
		*gained = focused
	}
	open := focused && (p.Open == nil || *p.Open) && (len(p.Suggestions) > 0 || p.Feedback != "")
	popup := ui.PopoverBase(c, row, &open, func(panel ui.Element) {
		minWidth := float32(280)
		if k.Skin == MacOS {
			minWidth = 420
			panel.AttachTo(row, ui.AnchorBottomEnd, ui.AnchorTopEnd).Top(6).Width(max(row.Bounds().W+60, minWidth))
		}
		k.fldPopup(panel, pal, max(row.Bounds().W, minWidth))
		k.Scroll(c.Key("suggestion-scroll")).MaxHeight(360).MinHeight(0).Children(func() {
			res.Selected = k.fldSuggestions(c, p.Suggestions, suggestCfg{
				label: p.Label, footer: p.SuggestionsFooter, feedback: p.Feedback, hi: hi,
			})
		})
	})
	if p.Open != nil && popup.Dismissed() {
		*p.Open = false
	}
	res.Element, res.Input = row, in
	res.Edited, res.Submitted = in.Changed(), in.Submitted()
	return res
}
