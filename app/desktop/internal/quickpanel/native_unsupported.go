//go:build !darwin && !windows && !linux

package quickpanel

func NewNative() Platform { return unsupportedPlatform{} }
