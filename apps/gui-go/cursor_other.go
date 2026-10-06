//go:build !darwin

package main

func cursorPosition() (x, y float64, ok bool) { return 0, 0, false }
