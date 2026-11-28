//go:build !bench_probe

package desktopui

func benchRegisterView(*viewState) {}
func benchNotifyView(*viewState)   {}
