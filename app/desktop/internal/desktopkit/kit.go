// Package desktopkit composes application controls from the native BoardUI library.
package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// Kit carries the application color scheme. Mutable state belongs to callers.
type Kit struct{ Dark bool }

func For(dark bool) Kit           { return Kit{Dark: dark} }
func (k Kit) native() boardui.Kit { return boardui.Kit{Dark: k.Dark} }

// Color accepts only canonical BoardUI token names.
func (k Kit) Color(name string) ui.Color { return boardui.Token(k.Dark, name) }
func (k Kit) Theme() *ui.Theme           { return k.native().Theme() }
func (k Kit) Apply(c *ui.Context) {
	if err := boardui.LoadFonts(); err != nil {
		panic(err)
	}
	k.native().Apply(c)
}
func (k Kit) ControlRadius() float32 { return boardui.Geometry("input", "medium").Radius }
