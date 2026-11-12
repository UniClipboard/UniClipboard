package board

import (
	"strconv"

	"golang.org/x/sys/windows"
)

var clipboardSequence = windows.NewLazySystemDLL("user32.dll").NewProc("GetClipboardSequenceNumber")

func clipboardCopyID() string {
	v, _, _ := clipboardSequence.Call()
	return strconv.FormatUint(uint64(v), 10)
}
