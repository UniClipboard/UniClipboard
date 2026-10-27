//go:build !darwin

package main

func (h *HostService) installLaunchOriginObserver() {}
func platformAutomaticLaunch() bool                 { return false }
