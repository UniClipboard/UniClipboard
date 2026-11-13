package engine

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/directory"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/internal/wire"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// pendingRecv is an admitted clip waiting for its body. Admission reserved
// storage, so an unclaimed grant must give it back when it expires.
type pendingRecv struct {
	adm   peerbackend.Admission
	parts []rpc.ClipPart
	hash  string
	timer *time.Timer
}

// rxState holds the grants of one backend; it has its own lock.
type rxState struct {
	mu sync.Mutex
	m  map[string]*pendingRecv
}

// OpenReceive decides on the clip header alone (sync-backend-contract §5).
func (b *peerProcBackend) OpenReceive(_ context.Context, o rpc.ReceiveOpen) (rpc.ReceiveGrant, error) {
	discard := rpc.ReceiveGrant{Decision: rpc.DecisionDiscard}
	b.am.Lock()
	b.sweepLocked(time.Now())
	s := b.sess[o.ConnectionID]
	if s == nil {
		b.am.Unlock()
		return discard, rpcErr(rpc.CodeNotFound)
	}
	if subtle.ConstantTimeCompare([]byte(s.token), []byte(o.SessionToken)) != 1 {
		b.am.Unlock()
		return discard, rpcErr(rpc.CodePermission)
	}
	peer, live := s.peerID, s.ended.IsZero()
	rev := s.trustRev
	b.am.Unlock()
	if cur, ok := b.trustRevision(peer); !live || !ok || cur != rev {
		return discard, nil
	}
	if !b.syncEnabled(peer) {
		return discard, nil
	}
	if err := checkClipLimits(o.Clip.Parts, b.e); err != nil {
		return discard, nil
	}
	var total int64
	parts := make([]receivePart, len(o.Clip.Parts))
	for i, p := range o.Clip.Parts {
		total += int64(p.Size)
		parts[i] = receivePart{file: p.Kind == "file", mime: p.MIME, size: int64(p.Size)}
	}
	adm, ok := b.e.admitReceive(receiveRequest{
		sourceApplication: o.Clip.SourceApplication, backend: b.id, source: DeviceID(peer), origin: o.Clip.Origin,
		transfer: o.Clip.WireID, view: o.Clip.Hash, total: total, parts: parts,
	})
	if !ok {
		return discard, nil
	}
	var raw [32]byte
	if _, err := rand.Read(raw[:]); err != nil {
		adm.Done(clip.Snapshot{}, "", false)
		return discard, rpcErr(rpc.CodeInternal)
	}
	id := hex.EncodeToString(raw[:])
	pr := &pendingRecv{adm: adm, parts: o.Clip.Parts, hash: o.Clip.Hash}
	pr.timer = time.AfterFunc(rpc.ReceiveOpenTTLMs*time.Millisecond, func() {
		if b.takeRecv(id) != nil {
			adm.Done(clip.Snapshot{}, "", false)
		}
	})
	b.rx.mu.Lock()
	if b.rx.m == nil {
		b.rx.m = map[string]*pendingRecv{}
	}
	b.rx.m[id] = pr
	b.rx.mu.Unlock()
	return rpc.ReceiveGrant{Decision: rpc.DecisionAccept, ReceiveID: &id}, nil
}

func (b *peerProcBackend) takeRecv(id string) *pendingRecv {
	b.rx.mu.Lock()
	defer b.rx.mu.Unlock()
	pr := b.rx.m[id]
	delete(b.rx.m, id)
	return pr
}

// Receive reads every part of an admitted clip and hands it to the pipeline.
func (b *peerProcBackend) Receive(ctx context.Context, receiveID string, parts plugin.ReceiveParts) (string, error) {
	pr := b.takeRecv(receiveID)
	if pr == nil {
		return "", rpcErr(rpc.CodeNotFound)
	}
	pr.timer.Stop()
	snap, dir, err := b.readClip(ctx, pr, parts)
	if err == nil && snap.Hash() != pr.hash {
		err = rpcErr(rpc.CodeInvalidState) // the content is not what its header promised
		b.e.logf(slog.LevelWarn, "dropped incoming content", "reason", "hash_mismatch")
	}
	if err != nil {
		pr.adm.Done(clip.Snapshot{}, "", false)
		if dir != "" {
			_ = os.RemoveAll(dir)
		}
		return "", err
	}
	pr.adm.Done(snap, dir, true)
	return rpc.OutcomeCommit, nil
}

func (b *peerProcBackend) readClip(ctx context.Context, pr *pendingRecv, parts plugin.ReceiveParts) (snap clip.Snapshot, dir string, err error) {
	defer func() {
		if err != nil && dir != "" {
			_ = os.RemoveAll(dir)
			dir = ""
		}
	}()
	for want := 0; want < len(pr.parts); want++ {
		idx, body, nerr := parts.Next(ctx)
		if nerr != nil {
			if errors.Is(nerr, io.EOF) {
				nerr = rpcErr(rpc.CodeInvalidState)
			}
			return snap, dir, nerr
		}
		if idx != want {
			_ = body.Close()
			return snap, dir, rpcErr(rpc.CodeInvalidState)
		}
		p := pr.parts[idx]
		if p.Kind == "file" {
			if dir == "" {
				if err = os.MkdirAll(pr.adm.Inbox, 0o700); err == nil {
					dir, err = os.MkdirTemp(pr.adm.Inbox, "clip-")
				}
			}
			var path, name string
			if err == nil {
				name = wire.SafeName(p.Name)
				var sub string
				if sub, err = os.MkdirTemp(dir, "f-"); err == nil {
					path = filepath.Join(sub, name)
					err = writeBody(path, body, int64(p.Size))
				}
			}
			_ = body.Close()
			if err != nil {
				return snap, dir, err
			}
			snap.Files = append(snap.Files, clip.File{Name: name, Path: path, Size: int64(p.Size)})
			continue
		}
		data, rerr := io.ReadAll(io.LimitReader(body, int64(p.Size)+1))
		_ = body.Close()
		if rerr != nil || int64(len(data)) != int64(p.Size) {
			return snap, dir, rpcErr(rpc.CodeInvalidState)
		}
		if p.MIME == directory.MIME {
			var sizes []int64
			for _, part := range pr.parts {
				if part.Kind == "file" {
					sizes = append(sizes, int64(part.Size))
				}
			}
			if _, err := directory.Decode(data, sizes); err != nil {
				return snap, dir, rpcErr(rpc.CodeInvalidArgument)
			}
		}
		snap.Parts = append(snap.Parts, clip.Part{Format: p.MIME, Data: data})
	}
	if _, _, nerr := parts.Next(ctx); !errors.Is(nerr, io.EOF) {
		return snap, dir, rpcErr(rpc.CodeInvalidState)
	}
	return snap, dir, nil
}

func writeBody(path string, r io.Reader, n int64) error {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	w, err := io.Copy(f, io.LimitReader(r, n+1))
	if cerr := f.Close(); err == nil {
		err = cerr
	}
	if err == nil && w != n {
		err = rpcErr(rpc.CodeInvalidState)
	}
	return err
}

// checkClipLimits rejects a clip over the protocol caps or the configured
// limits, judged on the header alone.
func checkClipLimits(parts []rpc.ClipPart, e *Engine) error {
	e.mu.Lock()
	lim := e.settings.Value.Limits
	e.mu.Unlock()
	var total int64
	manifestCount := 0
	for i, p := range parts {
		if p.MIME == directory.MIME {
			manifestCount++
			if i != 0 || p.Kind != "representation" {
				return rpcErr(rpc.CodeInvalidArgument)
			}
		}
	}
	if manifestCount > 1 {
		return rpcErr(rpc.CodeInvalidArgument)
	}
	if manifestCount == 1 {
		for _, p := range parts[1:] {
			if p.Kind != "file" {
				return rpcErr(rpc.CodeInvalidArgument)
			}
		}
	}
	for _, p := range parts {
		n := int64(p.Size)
		if p.MIME == directory.ReferenceMIME || (p.MIME == directory.MIME && (n <= 0 || n > directory.MaxManifest)) {
			return rpcErr(rpc.CodeInvalidArgument)
		}
		if n < 0 || (p.Kind == "file" && (n > wire.MaxFileBytes || n > lim.MaxFileBytes)) {
			return rpcErr(rpc.CodeLimitExceeded)
		}
		total += n
	}
	if total > wire.MaxTotal || total > lim.MaxTotalBytes {
		return rpcErr(rpc.CodeLimitExceeded)
	}
	return nil
}
