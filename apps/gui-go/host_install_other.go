//go:build !darwin && !windows && !linux

package main

import "github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"

func (h *HostService) installPayload([]byte, string) error { return update.ErrInstallUnsupported }

func (h *HostService) relaunchAfterInstall() error { return update.ErrInstallUnsupported }
