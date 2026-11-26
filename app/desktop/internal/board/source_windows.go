//go:build windows

package board

import "github.com/uniclipboard/unic/internal/nativeboard"

func nativeVersion() uint64 { return nativeboard.ClipboardOwnerRevision() }
func nativeSource() sourceState {
	revision, app := nativeboard.ClipboardSource()
	return sourceState{token: revision, app: app}
}
