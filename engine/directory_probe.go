//go:build !unic_e2e

package engine

import "context"

type directoryProbe struct{}

func (*directoryProbe) observe(context.Context, string) error { return nil }
