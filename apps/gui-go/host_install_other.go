//go:build !darwin && !windows && !linux

package main

import "os"

import "github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"

func (h *HostService) installPayload(*os.File, string) error { return update.ErrInstallUnsupported }

func (h *HostService) relaunchAfterInstall() error { return update.ErrInstallUnsupported }
