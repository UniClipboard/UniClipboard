package desktopkit

import (
	"runtime"
	"time"

	"github.com/uniclipboard/unic/app/desktop/boardui"

	"github.com/egoist/mygo/ui"
)

type searchDisclosureState struct{ Open, Request, WasFocused, ReturnFocus bool }

type SearchFieldToken struct {
	ID          string
	Field       string // the condition's field, already localized; display only
	Value       string
	Kind        SearchTokenKind
	RemoveLabel string // accessible name of the remove button, e.g. "Remove Tag #deploy"
}

type TokenSearchFieldProps struct {
	Toolbar bool
	// Unified renders the always-visible U1 centered omnibar combining live token search
	// with an integrated trailing filter funnel button and quick-filter popover.
	Unified          bool
	Label            string
	Placeholder      string
	ExternalShortcut bool
	ShortcutHint     string // display-only, e.g. "⌘F"
	Value            *string
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
	// FilterOpen, FilterCount, FilterLabel and FilterPanel power the integrated filter
	// button inside the U1 unified omnibar.
	FilterOpen  *bool
	FilterCount int
	FilterLabel string
	FilterPanel func()
	Disabled    bool
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

func (k Kit) TokenSearchField(c *ui.Context, p TokenSearchFieldProps) TokenSearchResult {
	pal := k.fldPal()
	{
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
	disclosure := p.Toolbar && !p.Unified
	if p.Toolbar {
		m := boardui.Geometry("input", "small")
		pal.h, pal.radius, pal.padX = m.Height, m.Radius, m.PaddingX
	}
	var state *searchDisclosureState
	width := float32(460)
	if p.Unified {
		state = ui.Local(row, "unified-focus", func() searchDisclosureState { return searchDisclosureState{} })
		if !p.Disabled && !p.ExternalShortcut {
			modifier := ui.Ctrl
			if runtime.GOOS == "darwin" {
				modifier = ui.Cmd
			}
			c.OnWindowShortcut(modifier, ui.KeyF, func() {
				state.Request = true
				if p.FilterOpen != nil {
					*p.FilterOpen = false
				}
			})
		}
		row.Grow(1).Height(32).MinWidth(100).MaxWidth(456).Padding(0, 4, 0, 10).Gap(6).Radius(9).Clip()
	} else if disclosure {
		state = ui.Local(row, "disclosure", func() searchDisclosureState { return searchDisclosureState{Open: active} })
		if active {
			state.Open = true
		}
		if !p.Disabled && !p.ExternalShortcut {
			modifier := ui.Ctrl
			if runtime.GOOS == "darwin" {
				modifier = ui.Cmd
			}
			c.OnWindowShortcut(modifier, ui.KeyF, func() { state.Open, state.Request = true, true })
		}
		target := float32(32)
		if state.Open {
			target = 460
		}
		width = row.AnimateTo("width", target, ui.Animation{Duration: 280 * time.Millisecond, Ease: ui.CubicBezier(.2, 0, 0, 1)})
		row.Padding(0).Gap(0).Height(32).Width(width).MinWidth(32).MaxWidth(width).Shrink(1).Radius(pal.radius).Clip()
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
		if p.Unified {
			in.MinWidth(44).Grow(1)
		} else {
			in.MinWidth(64)
		}
		if p.OnKey != nil || disclosure || p.Unified {
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
				if !handled && p.Unified && tk.Key == ui.KeyEscape {
					if p.FilterOpen != nil && *p.FilterOpen {
						*p.FilterOpen = false
						handled = true
					}
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
		if (disclosure || p.Unified) && state.Request && !p.Disabled {
			in.Focus()
			if in.Focused() {
				state.Request = false
			}
		}
		if !active && p.ShortcutHint != "" {
			if p.Unified {
				ui.Box(c.Key("hint")).Padding(1.5, 5).Radius(5).Background(k.surfaceSolid()).
					Border(1, k.Color("separator-border")).Shrink(0).AccessHidden(true).Children(func() {
					ui.Text(c, p.ShortcutHint).FontSize(10).FontWeight(500).TextColor(k.Color("text-tertiary")).SingleLine()
				})
			} else {
				ui.Text(c.Key("hint"), p.ShortcutHint).FontSize(pal.size - 2).TextColor(pal.hint)
			}
		}
		if active && p.ClearLabel != "" {
			clear := ui.ButtonBase(c.Key("clear")).Label(p.ClearLabel).Size(20, 20).Radius(10).Shrink(0).Center().
				Background(pal.fill).TextColor(pal.fg).KeepFocus()
			clear.Children(func() { k.Icon(c, IconDismiss, 10) })
			if clear.Clicked() {
				res.Cleared = true
			}
		}
		if p.Unified && p.FilterPanel != nil {
			ui.Box(c.Key("fsep")).Width(1).Height(14).Background(k.Color("separator-border")).Shrink(0).AccessHidden(true)
			flbl := p.FilterLabel
			if flbl == "" {
				flbl = "Filters"
			}
			fOpen := p.FilterOpen != nil && *p.FilterOpen
			fActive := p.FilterCount > 0 || fOpen
			fb := ui.ButtonBase(c.Key("filter-btn")).Label(flbl).Tooltip(flbl).Height(24).MinWidth(24).PaddingX(6).Radius(6).
				Gap(4).Center().Shrink(0)
			fInk := k.Color("text-secondary")
			if fActive {
				fb.Background(k.Color("accent-500").Alpha(.12))
				fInk = k.Color("accent-500")
			} else if fb.Hovered() {
				fb.Background(k.Color("background-tertiary-default"))
				fInk = k.Color("text-primary")
			}
			fb.Children(func() {
				k.Icon(c, IconFilter, 13).TextColor(fInk)
				if p.FilterCount > 0 {
					ui.Box(c.Key("fcnt")).MinWidth(15).Height(15).PaddingX(4).Radius(7.5).Center().
						Background(k.Color("accent-500")).AccessHidden(true).Children(func() {
						ui.Text(c, itoaPos(p.FilterCount)).FontSize(9.5).FontWeight(700).TextColor(k.Color("text-on-accent")).SingleLine()
					})
				}
			})
			if fb.Clicked() && p.FilterOpen != nil {
				*p.FilterOpen = !*p.FilterOpen
				if *p.FilterOpen && p.Open != nil {
					*p.Open = false
				}
			}
		}
	}
	row.Children(func() {
		glyph := func() { k.Icon(c.Key("glyph"), IconSearch, 14).TextColor(pal.fg2).Shrink(0) }
		if disclosure {
			trigger = ui.ButtonBase(c.Key("search-icon")).Label(p.Label).Size(32, 32).Shrink(0).KeepFocus().Disabled(p.Disabled).Center().TextColor(pal.fg2)
			trigger.Children(glyph)
			if trigger.Clicked() {
				state.Open, state.Request = true, true
			}
			ui.Row(c.Key("reveal")).Width(max(0, width-32)).Height(32).MinWidth(0).Shrink(1).Clip().AccessHidden(!state.Open).Children(func() {
				ui.Row(c.Key("input-body")).Width(428).MinWidth(0).Shrink(1).Gap(6).PaddingEnd(10).AlignItems(ui.Center).Children(body)
			})
		} else {
			{
				glyph()
			}
			body()

		}
	})
	if disclosure && state.ReturnFocus {
		trigger.Focus()
		state.ReturnFocus = false
	}
	focused := in.Focused()
	if p.Unified {
		fOpen := p.FilterOpen != nil && *p.FilterOpen
		if k.Dark {
			if focused || fOpen {
				row.Background(k.Color("background-secondary-default")).Border(1.25, k.Color("accent-500"))
			} else if row.Hovered() {
				row.Background(k.Color("background-primary-default").MixOklab(k.Color("background-quaternary-default"), .25)).Border(1, k.Color("border-button-hover"))
			} else {
				row.Background(k.Color("background-primary-default")).Border(1, k.Color("separator-border"))
			}
		} else {
			if focused || fOpen {
				row.Background(k.Color("background-primary-default")).Border(1.25, k.Color("accent-500"))
			} else if row.Hovered() {
				row.Background(k.Color("background-tertiary-default")).Border(1, k.Color("border-button-hover"))
			} else {
				row.Background(k.Color("background-secondary-default")).Border(1, k.Color("separator-border"))
			}
		}
	} else {
		k.fldStyle(row, pal, focused, false, p.Disabled)
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

	// Menu visibility. In Unified mode, bare focus never opens the legacy full
	// suggestion list (filters live in the integrated FilterPanel popover); only
	// explicit prefix-driven suggestions or non-unified fields open on focus.
	if p.Open != nil {
		gained := ui.Local(in, "desktopkit.focus", func() bool { return false })
		if !p.Unified && (focused && !*gained || (in.Changed() && p.OnKey == nil)) {
			*p.Open = true
		}
		if p.Unified && focused && !*gained && p.FilterOpen != nil {
			*p.FilterOpen = false
		}
		if !focused {
			*p.Open = false
		}
		*gained = focused
	}
	showSuggestions := focused && (p.Open == nil || *p.Open) && (len(p.Suggestions) > 0 || p.Feedback != "")
	showFilter := p.Unified && p.FilterPanel != nil && p.FilterOpen != nil && *p.FilterOpen && !showSuggestions
	open := showSuggestions || showFilter
	popup := ui.PopoverBase(c, row, &open, func(panel ui.Element) {
		minWidth := float32(280)
		if showFilter {
			minWidth = 356
		}
		k.fldPopup(panel, pal, max(row.Bounds().W, minWidth))
		if showFilter {
			p.FilterPanel()
		} else {
			k.Scroll(c.Key("suggestion-scroll")).MaxHeight(360).MinHeight(0).Children(func() {
				res.Selected = k.fldSuggestions(c, p.Suggestions, suggestCfg{
					label: p.Label, footer: p.SuggestionsFooter, feedback: p.Feedback, hi: hi,
				})
			})
		}
	})
	if popup.Dismissed() {
		if p.Open != nil {
			*p.Open = false
		}
		if p.FilterOpen != nil {
			*p.FilterOpen = false
		}
	}
	res.Element, res.Input = row, in
	res.Edited, res.Submitted = in.Changed(), in.Submitted()
	return res
}
