//go:build !darwin

package main

import "context"

func watchSystemWake(context.Context, string) func() { return func() {} }
