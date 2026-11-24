package mobile

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/uniclipboard/unic/engine"
)

// CommitSharedItem consumes a host-owned share staging directory. The native
// host resolves the directory from its private inbox, never from a JS path.
func (m *Engine) CommitSharedItem(dir, deviceIDsJSON string) (string, error) {
	ctx := context.Background()
	var devices []engine.DeviceID
	if deviceIDsJSON != "" {
		if err := json.Unmarshal([]byte(deviceIDsJSON), &devices); err != nil || len(devices) == 0 {
			return "", wrapErr(&engine.Error{Code: engine.CodeInvalidArgument, Message: "select at least one device"})
		}
	}

	requestID := engine.RequestID("mobile-share-" + filepath.Base(dir))
	if existing, err := m.e.GetSubmission(ctx, requestID); err == nil && existing.Phase == engine.SubmissionCommitted {
		return marshalJSON(existing)
	}

	manifest, err := os.Open(filepath.Join(dir, "entry.json"))
	if err != nil {
		return "", wrapErr(err)
	}
	var item struct {
		Kind string `json:"kind"`
		Text string `json:"text"`
		Name string `json:"name"`
		MIME string `json:"mime"`
	}
	err = json.NewDecoder(io.LimitReader(manifest, 2<<20)).Decode(&item)
	_ = manifest.Close()
	if err != nil {
		return "", wrapErr(err)
	}

	in := engine.ContentInput{}
	switch item.Kind {
	case "text":
		in.Representations = []engine.RepresentationInput{{
			MIME: engine.MimeText, Size: int64(len(item.Text)), Reader: strings.NewReader(item.Text),
		}}
	case "file":
		file, err := os.Open(filepath.Join(dir, "payload"))
		if err != nil {
			return "", wrapErr(err)
		}
		defer file.Close()
		info, err := file.Stat()
		if err != nil {
			return "", wrapErr(err)
		}
		if !info.Mode().IsRegular() {
			return "", wrapErr(errors.New("invalid share payload"))
		}
		if item.MIME == engine.MimePNG {
			in.Representations = []engine.RepresentationInput{{
				MIME: item.MIME, Size: info.Size(), Reader: file,
			}}
		} else {
			in.Files = []engine.FileInput{{
				Name: item.Name, MIME: item.MIME, Size: info.Size(), Reader: file,
			}}
		}
	default:
		return "", wrapErr(errors.New("invalid share item type"))
	}

	handle, err := m.e.ImportContent(ctx, in)
	if err != nil {
		return "", wrapErr(err)
	}
	defer func() { _ = m.e.ReleaseContent(ctx, handle.LeaseID) }()
	result, err := m.e.CommitContent(ctx, engine.CommitRequest{
		RequestID: requestID,
		LeaseID:   handle.LeaseID,
		DeviceIDs: devices,
		LocalOnly: devices == nil,
	})
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(result)
}
