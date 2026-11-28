//go:build !darwin && !windows

package sampler

import (
	"context"
	"runtime"
)

func sampleProcessNative(int) (ProcessSample, error) {
	return ProcessSample{}, ErrUnsupportedPlatform
}

func captureAttributionNative(_ context.Context, _ string, _ map[string]int) (*AttributionSnapshot, error) {
	return nil, ErrUnsupportedPlatform
}

func clipboardSequenceNative() (uint64, error) {
	return 0, ErrUnsupportedPlatform
}

func hostArchNative() string {
	return runtime.GOARCH
}

func fillHostInfoNative(_ context.Context, _ *HostInfo) {}
