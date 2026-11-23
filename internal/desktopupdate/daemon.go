package desktopupdate

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"sync"

	api "github.com/uniclipboard/unic/internal/localapi"
)

// DaemonSource retrieves verified release artifacts from the profile's unicd.
type DaemonSource struct {
	Client         *api.Client
	CurrentVersion string
	Key            string

	mu       sync.Mutex
	token    string
	artifact Artifact
}

func (s *DaemonSource) Manifest(ctx context.Context, _ bool) (io.ReadCloser, error) {
	selection, err := s.Client.CheckDesktopUpdate(ctx, api.DesktopUpdateRequest{
		CurrentVersion: s.CurrentVersion, PublicKey: s.Key,
	})
	if err != nil {
		return nil, err
	}
	var data struct {
		URL       string `json:"url"`
		Size      int64  `json:"size"`
		SHA256    string `json:"sha256"`
		Signature string `json:"signature"`
	}
	if selection.Token == "" || json.Unmarshal(selection.Manifest, &data) != nil || data.URL == "" {
		return nil, errors.New("invalid daemon update selection")
	}
	s.mu.Lock()
	s.token = selection.Token
	s.artifact = Artifact{DownloadURL: data.URL, Size: data.Size, SHA256: data.SHA256, Signature: data.Signature}
	s.mu.Unlock()
	return io.NopCloser(bytes.NewReader(selection.Manifest)), nil
}

func (s *DaemonSource) Open(ctx context.Context, address string) (io.ReadCloser, error) {
	s.mu.Lock()
	token, artifact := s.token, s.artifact
	s.mu.Unlock()
	if token == "" || artifact.DownloadURL != address {
		return nil, errors.New("update archive was not selected")
	}
	body, err := s.Client.OpenDesktopUpdate(ctx, token)
	if err != nil {
		return nil, err
	}
	defer body.Close()
	// Complete and verify the handoff before the installer retires unicd.
	return copyVerified(body, artifact, s.Key)
}
