package quickpanel

import "github.com/uniclipboard/unic/internal/clipboardintegration"

type Clipboard = clipboardintegration.TrackedBoard

func NewClipboard(board clipboardintegration.Board) *Clipboard {
	return clipboardintegration.NewTrackedBoard(board)
}
