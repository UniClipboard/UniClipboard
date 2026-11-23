//go:build darwin

package board

import "github.com/uniclipboard/unic/internal/nativeboard"

func nativeVersion() uint64 { return nativeboard.Version() }
func nativeSource() sourceState {
	token, app := nativeboard.Source()
	return sourceState{token: token, app: app}
}
