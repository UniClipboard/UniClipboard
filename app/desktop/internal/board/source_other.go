//go:build !darwin && !windows && !linux

package board

func nativeVersion() uint64     { return 0 }
func nativeSource() sourceState { return sourceState{} }
