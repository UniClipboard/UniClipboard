package uikit

// Component API conventions (all agents follow these):
//
//   - Every component is a method on Kit: func (k Kit) Name(c *ui.Context, p NameProps, ...) ui.Element.
//     Props are a struct named NameProps; zero values are sensible defaults.
//   - Immediate mode: there are no callbacks. Read interaction from the returned element
//     (e.Clicked()) or pass state by pointer (*bool, *string, *int) like mygo's own widgets.
//   - Containers take their content as a trailing `body func()` and build it inside
//     Children(body); give list items stable keys with c.Key(id).
//   - Use Kit.Scroll for vertical scrolling instead of ui.Scroll. It shares the
//     indicator visibility policy with Kit lists and grids. Keep timing and
//     hover rules in scroll_indicator.go, never in pages or individual components.
//   - Switch renders only its track and thumb. Put visible captions in the
//     enclosing row; SwitchProps.Label supplies only the accessible name.
//   - Build on mygo's *Base widgets (ButtonBase, TextInputBase, SwitchBase, TabsBase,
//     DialogBase, MenuBase ...) so keyboard, focus and accessibility keep working; add the
//     look on top with Background/Border/Shadow/InsetShadow/Draw. See
//     /Users/LWP1/Work/letterdock/third_party/solid-glass-mygo for a worked example.
//   - On macOS, Select and uncontrolled Menu/ContextMenu use mygo's system menu
//     API. Let the OS own menu appearance, tracking, and animation. An explicit
//     Open pointer retains the caller-controlled in-window overlay contract.
//   - Looks come from tokens: k.Sem("primary") for shared semantics, k.Color("fill-2") /
//     k.Px("...") for skin tokens (names differ per skin; see tokens_gen.go). Branch on
//     k.Skin (Linux/MacOS/Windows) inside a component for structural differences.
//   - Respect the reference TSX in the three skins for geometry, states and motion.
//   - Test headlessly with ui.NewTester(view, w, h) (Click, Key, ...), once per skin x scheme
//     (Kit{Skin: s, Dark: d}); see solid-glass-mygo/controls_test.go.
