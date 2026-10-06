package main

import (
	"context"
	"encoding/json"
	"errors"
)

var errSyncRejected = errors.New("sync settings update was rejected")

func parseSyncEnabled(raw []byte) (bool, bool) {
	var settings struct {
		Sync struct {
			SyncEnabled bool `json:"syncEnabled"`
		} `json:"sync"`
	}
	if err := json.Unmarshal(raw, &settings); err != nil {
		return false, false
	}
	return settings.Sync.SyncEnabled, true
}

func (h *HostService) readSyncEnabled(ctx context.Context) (bool, error) {
	var raw json.RawMessage
	if err := h.client.Get(ctx, "/settings", &raw); err != nil {
		return false, err
	}
	enabled, ok := parseSyncEnabled(raw)
	if !ok {
		return false, errors.New("unreadable settings response")
	}
	return enabled, nil
}
