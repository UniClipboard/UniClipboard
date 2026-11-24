//go:build !linux

package main

import "github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"

func setRoleProcessName(desktopprocess.Role) {}
