package engine

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/relayclient"
	"github.com/uniclipboard/unic/internal/relaycrypto"
	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/internal/wire"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

const (
	defaultRelayMaxBytes      = 25 << 20 // 25 MiB official default
	relayUploadConcurrency    = 3
	maxActiveRelayAssemblies  = 16
	relayCleanupTimeout       = 2 * time.Second
	relayChunkWaitPoll        = 400 * time.Millisecond
	relayReconnectMinBackoff  = 500 * time.Millisecond
	relayReconnectMaxBackoff  = 30 * time.Second
)

// relayCoordinator manages E2EE HTTP relay client lifecycle, per-peer delivery
// token registration, SSE mailbox subscription, inbound chunked assembly and
// outbound zero-wait relay uploads (decision 0035).
type relayCoordinator struct {
	e *Engine

	mu        sync.Mutex
	active    bool
	origin    string
	accessKey string
	maxBytes  int64
	client    *relayclient.Client
	mailboxID string

	cancel     context.CancelFunc
	wg         sync.WaitGroup
	registered map[string]bool // origin + ":" + tokenHash
	assemblies map[string]*relayAssembly
}

type relayAssembly struct {
	transferID string
	senderID   string
	cancel     context.CancelFunc
	mu         sync.Mutex
	available  []byte
	chunkWake  chan struct{}
}

func newRelayCoordinator(e *Engine) *relayCoordinator {
	return &relayCoordinator{
		e:          e,
		registered: map[string]bool{},
		assemblies: map[string]*relayAssembly{},
	}
}

func (r *relayCoordinator) effectiveConfig() (enabled bool, urlStr, accessKey string, maxBytes int64) {
	r.e.mu.Lock()
	up := r.e.netUp && r.e.life != LifecycleStopping && r.e.life != LifecycleStopped && r.e.life != LifecycleClosed
	s := r.e.settings.Value
	r.e.mu.Unlock()
	if !up || !s.Sync.Enabled || !s.Sync.Relay.Enabled {
		return false, "", "", 0
	}
	u := strings.TrimSpace(s.Sync.Relay.URL)
	if u == "" && r.e.opts.relayClient != nil {
		u = r.e.opts.relayClient.Origin()
	}
	if u == "" {
		u = relayclient.DefaultURL
	}
	mb := s.Sync.Relay.MaxBytes
	if mb <= 0 {
		mb = defaultRelayMaxBytes
	}
	if s.Limits.MaxTotalBytes > 0 && mb > s.Limits.MaxTotalBytes {
		mb = s.Limits.MaxTotalBytes
	}
	return true, u, s.Sync.Relay.AccessKey, mb
}

// isEnabled reports whether relay fallback is configured and active right now.
func (r *relayCoordinator) isEnabled() bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.active && r.client != nil
}

func (r *relayCoordinator) snapshotClient() (*relayclient.Client, string, int64, bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if !r.active || r.client == nil {
		return nil, "", 0, false
	}
	return r.client, r.mailboxID, r.maxBytes, true
}

// reconcile aligns the coordinator with current Engine runtime/settings.
// Stopping waits for SSE and active downloads to finish (revocation/suspension barrier).
func (r *relayCoordinator) reconcile() {
	enabled, rawURL, accessKey, maxBytes := r.effectiveConfig()
	if !enabled {
		r.stop()
		return
	}
	var cli *relayclient.Client
	if r.e.opts.relayClient != nil && (rawURL == "" || rawURL == r.e.opts.relayClient.Origin()) {
		cli = r.e.opts.relayClient
	} else {
		var err error
		cli, err = relayclient.New(relayclient.Config{BaseURL: rawURL, AccessKey: accessKey})
		if err != nil {
			r.stop()
			return
		}
	}
	origin := cli.Origin()
	mailboxID, err := relaycrypto.DeriveMailboxID(origin, r.e.ident.Public())
	if err != nil {
		r.stop()
		return
	}

	r.mu.Lock()
	same := r.active && r.origin == origin && r.accessKey == accessKey && r.client != nil
	if same {
		changedMax := r.maxBytes != maxBytes
		r.maxBytes = maxBytes
		r.mu.Unlock()
		if changedMax {
			r.pushProfiles()
		}
		return
	}
	oldCancel := r.cancel
	r.active = false
	r.cancel = nil
	r.mu.Unlock()

	if oldCancel != nil {
		oldCancel()
	}
	r.wg.Wait()

	r.e.mu.Lock()
	baseCtx := r.e.runCtx
	r.e.mu.Unlock()
	if baseCtx == nil {
		return
	}
	ctx, cancel := context.WithCancel(baseCtx)
	r.mu.Lock()
	r.active = true
	r.origin = origin
	r.accessKey = accessKey
	r.maxBytes = maxBytes
	r.client = cli
	r.mailboxID = mailboxID
	r.cancel = cancel
	r.registered = map[string]bool{}
	r.wg.Add(1)
	r.mu.Unlock()

	go func() {
		defer r.wg.Done()
		r.runLoop(ctx, cli, mailboxID, origin)
	}()
	r.pushProfiles()
}

// stop cancels the SSE subscription and all active inbound relay assemblies and waits.
func (r *relayCoordinator) stop() {
	r.mu.Lock()
	cancel := r.cancel
	r.active = false
	r.cancel = nil
	r.client = nil
	r.origin = ""
	r.mailboxID = ""
	var asmCancels []context.CancelFunc
	for _, a := range r.assemblies {
		if a.cancel != nil {
			asmCancels = append(asmCancels, a.cancel)
		}
	}
	r.mu.Unlock()

	if cancel != nil {
		cancel()
	}
	for _, c := range asmCancels {
		c()
	}
	r.wg.Wait()
}

func (r *relayCoordinator) pushProfiles() {
	r.e.mu.Lock()
	procs := append([]*peerProcBackend(nil), r.e.peerProcs...)
	r.e.mu.Unlock()
	for _, pp := range procs {
		if bridge, ok := pp.pb.(plugin.PeerRelayBridge); ok {
			bridge.PushRelayProfiles()
		}
	}
}

// provideRelayProfile builds/loads a per-peer delivery token for peerID and
// ensures its hash is registered on the relay server before advertising it.
func (r *relayCoordinator) provideRelayProfile(peerID string) (plugin.RelayProfile, bool) {
	cli, mailboxID, maxBytes, ok := r.snapshotClient()
	if !ok {
		return plugin.RelayProfile{}, false
	}
	d, err := r.e.st.Device(peerID)
	if err != nil || d.Trust != store.TrustTrusted {
		return plugin.RelayProfile{}, false
	}
	origin := cli.Origin()
	tok, err := r.e.st.RelayToken(peerID)
	if err != nil || tok.RelayOrigin != origin || tok.DeliveryToken == "" {
		rawTok, gerr := relaycrypto.NewDeliveryToken()
		if gerr != nil {
			return plugin.RelayProfile{}, false
		}
		epoch := uint64(1)
		if err == nil && tok.Epoch > 0 {
			epoch = tok.Epoch + 1
		}
		tok = store.RelayToken{
			DeviceID:      peerID,
			RelayOrigin:   origin,
			DeliveryToken: rawTok,
			TokenHash:     relaycrypto.TokenHash(rawTok),
			Epoch:         epoch,
			CreatedMs:     time.Now().UnixMilli(),
		}
		if uerr := r.e.st.UpsertRelayToken(tok); uerr != nil {
			return plugin.RelayProfile{}, false
		}
	}

	regKey := origin + ":" + tok.TokenHash
	r.mu.Lock()
	already := r.registered[regKey]
	r.mu.Unlock()
	if !already {
		regCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		err := cli.RegisterSender(regCtx, r.e.ident, mailboxID, tok.TokenHash, maxBytes)
		cancel()
		if err != nil {
			r.e.logf(slog.LevelWarn, "relay sender registration failed")
			return plugin.RelayProfile{}, false
		}
		r.mu.Lock()
		if r.origin == origin {
			r.registered[regKey] = true
		}
		r.mu.Unlock()
	}

	return plugin.RelayProfile{
		RelayOrigin:   origin,
		MailboxID:     mailboxID,
		DeliveryToken: tok.DeliveryToken,
		MaxBytes:      maxBytes,
		Epoch:         tok.Epoch,
	}, true
}

// onPeerRelayProfile stores a validated relay route received from a trusted peer.
func (r *relayCoordinator) onPeerRelayProfile(peerID string, prof plugin.RelayProfile) {
	cli, _, _, ok := r.snapshotClient()
	if !ok {
		return
	}
	d, err := r.e.st.Device(peerID)
	if err != nil || d.Trust != store.TrustTrusted {
		return
	}
	// SSRF guard: only accept routes matching our configured relay origin.
	if err := cli.ValidatePeerOrigin(prof.RelayOrigin); err != nil {
		r.e.logf(slog.LevelWarn, "relay peer profile origin mismatch")
		return
	}
	normOrigin, err := relaycrypto.NormalizeRelayOrigin(prof.RelayOrigin)
	if err != nil {
		return
	}
	if err := r.e.st.UpsertRelayRoute(store.RelayRoute{
		DeviceID:      peerID,
		RelayOrigin:   normOrigin,
		MailboxID:     prof.MailboxID,
		DeliveryToken: prof.DeliveryToken,
		MaxBytes:      prof.MaxBytes,
		Epoch:         prof.Epoch,
		UpdatedMs:     time.Now().UnixMilli(),
	}); err != nil {
		return
	}
	r.e.deviceChanged(peerID)
}

// routeReady reports whether peerID has a valid stored relay route compatible
// with the current active relay origin.
func (r *relayCoordinator) routeReady(peerID string) bool {
	cli, _, _, ok := r.snapshotClient()
	if !ok {
		return false
	}
	route, err := r.e.st.RelayRoute(peerID)
	if err != nil {
		return false
	}
	return cli.ValidatePeerOrigin(route.RelayOrigin) == nil && route.MailboxID != "" && route.DeliveryToken != ""
}

// revokePeer cancels active inbound downloads from peerID, removes stored route
// and token, and best-effort revokes the sender token hash on the relay server.
func (r *relayCoordinator) revokePeer(deviceID string, dropToken bool) {
	r.mu.Lock()
	var cancels []context.CancelFunc
	for _, a := range r.assemblies {
		if a.senderID == deviceID && a.cancel != nil {
			cancels = append(cancels, a.cancel)
		}
	}
	r.mu.Unlock()
	for _, c := range cancels {
		c()
	}
	if !dropToken {
		return
	}
	_ = r.e.st.DeleteRelayRoute(deviceID)
	tok, err := r.e.st.DeleteRelayToken(deviceID)
	if err != nil {
		return
	}
	cli, mailboxID, _, ok := r.snapshotClient()
	if !ok || cli.Origin() != tok.RelayOrigin {
		return
	}
	r.mu.Lock()
	delete(r.registered, tok.RelayOrigin+":"+tok.TokenHash)
	r.mu.Unlock()
	revCtx, cancel := context.WithTimeout(context.Background(), relayCleanupTimeout)
	defer cancel()
	_ = cli.RevokeSender(revCtx, r.e.ident, mailboxID, tok.TokenHash)
}

// ---- inbound mailbox loop & chunked assembly ----

func (r *relayCoordinator) runLoop(ctx context.Context, cli *relayclient.Client, mailboxID, origin string) {
	r.ensureRegisteredTokens(ctx, cli, mailboxID, origin)

	var lastEventID string
	backoff := relayReconnectMinBackoff
	for ctx.Err() == nil {
		// Catch up any transfers uploaded while offline/reconnecting.
		r.pollPending(ctx, cli, mailboxID, origin)
		if ctx.Err() != nil {
			return
		}

		connectedAt := time.Now()
		err := cli.SubscribeEvents(ctx, r.e.ident, mailboxID, lastEventID, func(ev relayclient.SSEEvent) error {
			if ev.ID != "" {
				lastEventID = ev.ID
			}
			r.handleSSEEvent(ctx, cli, mailboxID, origin, ev)
			return nil
		})
		if ctx.Err() != nil {
			return
		}
		if time.Since(connectedAt) > 10*time.Second {
			backoff = relayReconnectMinBackoff
		}
		_ = err
		t := time.NewTimer(backoff)
		select {
		case <-ctx.Done():
			t.Stop()
			return
		case <-t.C:
		}
		if backoff < relayReconnectMaxBackoff {
			backoff *= 2
			if backoff > relayReconnectMaxBackoff {
				backoff = relayReconnectMaxBackoff
			}
		}
	}
}

func (r *relayCoordinator) ensureRegisteredTokens(ctx context.Context, cli *relayclient.Client, mailboxID, origin string) {
	toks, err := r.e.st.RelayTokens()
	if err != nil {
		return
	}
	_, _, maxBytes, _ := r.snapshotClient()
	for _, tok := range toks {
		if ctx.Err() != nil {
			return
		}
		if tok.RelayOrigin != origin {
			continue
		}
		d, derr := r.e.st.Device(tok.DeviceID)
		if derr != nil || d.Trust != store.TrustTrusted {
			continue
		}
		regCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
		rerr := cli.RegisterSender(regCtx, r.e.ident, mailboxID, tok.TokenHash, maxBytes)
		cancel()
		if rerr == nil {
			r.mu.Lock()
			if r.origin == origin {
				r.registered[origin+":"+tok.TokenHash] = true
			}
			r.mu.Unlock()
		}
	}
}

func (r *relayCoordinator) pollPending(ctx context.Context, cli *relayclient.Client, mailboxID, origin string) {
	listCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	summaries, err := cli.ListTransfers(listCtx, r.e.ident, mailboxID)
	cancel()
	if err != nil {
		return
	}
	for _, sum := range summaries {
		if ctx.Err() != nil {
			return
		}
		bm, berr := relaycrypto.DecodeBitmap(sum.AvailableBitmap, sum.Envelope.TotalChunks)
		if berr != nil {
			bm, _ = relaycrypto.NewBitmap(sum.Envelope.TotalChunks)
		}
		r.admitAssembly(ctx, cli, mailboxID, origin, sum.Envelope, bm)
	}
}

func (r *relayCoordinator) handleSSEEvent(ctx context.Context, cli *relayclient.Client, mailboxID, origin string, ev relayclient.SSEEvent) {
	switch ev.Event {
	case "transfer_single_ready", "transfer_announced":
		if ev.Envelope == nil {
			return
		}
		bm, err := relaycrypto.DecodeBitmap(ev.AvailableBitmap, ev.Envelope.TotalChunks)
		if err != nil {
			bm, _ = relaycrypto.NewBitmap(ev.Envelope.TotalChunks)
		}
		r.admitAssembly(ctx, cli, mailboxID, origin, *ev.Envelope, bm)
	case "chunk_ready":
		if ev.TransferID == "" {
			return
		}
		r.mu.Lock()
		asm := r.assemblies[ev.TransferID]
		r.mu.Unlock()
		if asm == nil {
			// Announced before we connected; poll to pick up envelope.
			go r.pollPending(ctx, cli, mailboxID, origin)
			return
		}
		asm.mu.Lock()
		if ev.ChunkIndex != nil {
			relaycrypto.SetBit(asm.available, *ev.ChunkIndex)
		}
		asm.mu.Unlock()
		select {
		case asm.chunkWake <- struct{}{}:
		default:
		}
	}
}

func (r *relayCoordinator) admitAssembly(parentCtx context.Context, cli *relayclient.Client, mailboxID, origin string, env relaycrypto.Envelope, initialBM []byte) {
	// Step 1: Validate sender trust and authorization before any decryption or disk I/O.
	d, err := r.e.st.Device(env.SenderID)
	if err != nil || d.Trust != store.TrustTrusted {
		return
	}
	backendID, allowed := r.e.authorizedPeerBackend(env.SenderID)
	if !allowed {
		return
	}
	ve, err := relaycrypto.VerifyEnvelope(origin, env, r.e.ident.DeviceID(), d.PublicKey, time.Now())
	if err != nil {
		return
	}
	r.e.mu.Lock()
	lim := r.e.settings.Value.Limits
	syncOn := r.e.settings.Value.Sync.Enabled
	r.e.mu.Unlock()
	if !syncOn {
		return
	}
	// Ciphertext includes GCM tag per chunk; reject oversized envelopes up front.
	maxAllowedCT := lim.MaxTotalBytes + int64(wire.MaxControl) + int64(env.TotalChunks)*relaycrypto.GCMTagSize
	if env.TotalCiphertextBytes > maxAllowedCT {
		delCtx, cancel := context.WithTimeout(context.Background(), relayCleanupTimeout)
		_ = cli.DeleteTransferAsOwner(delCtx, r.e.ident, mailboxID, env.TransferID)
		cancel()
		return
	}
	cek, err := relaycrypto.OpenCEK(r.e.ident, origin, ve)
	if err != nil {
		return
	}

	r.mu.Lock()
	if !r.active {
		r.mu.Unlock()
		return
	}
	if existing := r.assemblies[env.TransferID]; existing != nil {
		existing.mu.Lock()
		for i := 0; i < env.TotalChunks; i++ {
			if relaycrypto.HasBit(initialBM, i) {
				relaycrypto.SetBit(existing.available, i)
			}
		}
		existing.mu.Unlock()
		select {
		case existing.chunkWake <- struct{}{}:
		default:
		}
		r.mu.Unlock()
		return
	}
	if len(r.assemblies) >= maxActiveRelayAssemblies {
		r.mu.Unlock()
		return
	}
	asmCtx, asmCancel := context.WithCancel(parentCtx)
	asm := &relayAssembly{
		transferID: env.TransferID,
		senderID:   env.SenderID,
		cancel:     asmCancel,
		available:  append([]byte(nil), initialBM...),
		chunkWake:  make(chan struct{}, 1),
	}
	r.assemblies[env.TransferID] = asm
	r.wg.Add(1)
	r.mu.Unlock()

	go func() {
		defer r.wg.Done()
		defer func() {
			asmCancel()
			r.mu.Lock()
			if r.assemblies[env.TransferID] == asm {
				delete(r.assemblies, env.TransferID)
			}
			r.mu.Unlock()
		}()
		r.downloadAndCommit(asmCtx, asm, cli, mailboxID, backendID, ve, cek)
	}()
}

func (r *relayCoordinator) downloadAndCommit(
	ctx context.Context,
	asm *relayAssembly,
	cli *relayclient.Client,
	mailboxID string,
	backendID PluginID,
	ve relaycrypto.VerifiedEnvelope,
	cek []byte,
) {
	env := ve.Envelope
	cc, err := relaycrypto.NewChunkCipher(cek, env.TransferID, env.SenderID, env.RecipientID, env.TotalChunks)
	if err != nil {
		return
	}

	var discarded atomic.Bool
	dlCtx, dlCancel := context.WithCancel(ctx)
	defer dlCancel()
	if exp := time.UnixMilli(env.ExpiresMs); !exp.IsZero() {
		var tCancel context.CancelFunc
		dlCtx, tCancel = context.WithDeadline(dlCtx, exp)
		defer tCancel()
	}

	estPlain := max(env.TotalCiphertextBytes-int64(env.TotalChunks)*relaycrypto.GCMTagSize, 0)
	opID := r.e.startReceiveOp(ReceiveOperation{
		BackendID:   backendID,
		PeerID:      DeviceID(env.SenderID),
		State:       ReceiveReceiving,
		ContentKind: "unknown",
		Progress: ReceiveProgress{
			TotalBytes: estPlain,
			PartCount:  env.TotalChunks,
		},
	}, func() {
		discarded.Store(true)
		dlCancel()
	})

	// Stage decrypted plaintext stream in memory (single small chunk) or temp file under InboxDir/unic-recv.
	var memBuf bytes.Buffer
	var stageFile *os.File
	var stagePath string
	if env.TotalChunks > 1 {
		if err := os.MkdirAll(r.e.recvDir(), 0o700); err != nil {
			r.e.finishReceiveOp(opID, ReceiveFailed, err)
			return
		}
		f, ferr := os.CreateTemp(r.e.recvDir(), "relay-*")
		if ferr != nil {
			r.e.finishReceiveOp(opID, ReceiveFailed, ferr)
			return
		}
		stageFile = f
		stagePath = f.Name()
		defer func() {
			_ = stageFile.Close()
			_ = os.Remove(stagePath)
		}()
	}

	maxChunkCT := int64(env.ChunkSize) + relaycrypto.GCMTagSize
	var totalPlain int64
	for i := 0; i < env.TotalChunks; i++ {
		if err := r.waitChunkAvailable(dlCtx, asm, cli, mailboxID, env.TransferID, env.TotalChunks, i); err != nil {
			r.handleAssemblyAbort(opID, discarded.Load(), cli, mailboxID, env.TransferID, err)
			return
		}
		ct, derr := cli.DownloadChunk(dlCtx, r.e.ident, mailboxID, env.TransferID, i, maxChunkCT)
		if derr != nil {
			r.handleAssemblyAbort(opID, discarded.Load(), cli, mailboxID, env.TransferID, derr)
			return
		}
		pt, oerr := cc.OpenChunk(i, ct, ve.ChunkHashes[i])
		if oerr != nil {
			r.handleAssemblyAbort(opID, discarded.Load(), cli, mailboxID, env.TransferID, oerr)
			return
		}
		if stageFile != nil {
			if _, werr := stageFile.Write(pt); werr != nil {
				r.handleAssemblyAbort(opID, discarded.Load(), cli, mailboxID, env.TransferID, werr)
				return
			}
		} else {
			memBuf.Write(pt)
		}
		totalPlain += int64(len(pt))
		r.e.updateReceiveProgress(opID, totalPlain, i)
	}

	if discarded.Load() {
		r.handleAssemblyAbort(opID, true, cli, mailboxID, env.TransferID, errReceiveDiscarded)
		return
	}

	var plainReader io.Reader
	if stageFile != nil {
		if _, serr := stageFile.Seek(0, io.SeekStart); serr != nil {
			r.e.finishReceiveOp(opID, ReceiveFailed, serr)
			return
		}
		plainReader = stageFile
	} else {
		plainReader = bytes.NewReader(memBuf.Bytes())
	}

	clipMsg, herr := relaycrypto.ReadPlaintextHeader(plainReader)
	if herr != nil || clipMsg.ID != env.TransferID || clipMsg.Origin != env.SenderID {
		r.e.finishReceiveOp(opID, ReceiveFailed, newErr(CodeInvalidArgument, "malformed relay clip header"))
		r.bestEffortDeleteOwner(cli, mailboxID, env.TransferID)
		return
	}

	// Build rpc.ClipPart list for shared limit & admission checks.
	rpcParts := make([]rpc.ClipPart, len(clipMsg.Parts))
	recvParts := make([]receivePart, len(clipMsg.Parts))
	var declaredTotal int64
	for i, p := range clipMsg.Parts {
		declaredTotal += p.Size
		cp := rpc.ClipPart{Index: i, Size: rpc.U64(p.Size)}
		if p.Kind == "file" {
			cp.Kind, cp.Name = "file", p.Name
		} else {
			cp.Kind, cp.MIME = "representation", p.Name
		}
		rpcParts[i] = cp
		recvParts[i] = receivePart{file: p.Kind == "file", mime: cp.MIME, size: p.Size}
	}
	if err := checkClipLimits(rpcParts, r.e); err != nil {
		r.e.finishReceiveOp(opID, ReceiveDiscarded, nil)
		r.bestEffortAck(cli, mailboxID, env.TransferID)
		return
	}
	// Re-check peer authorization immediately before admission.
	if _, ok := r.e.authorizedPeerBackend(env.SenderID); !ok {
		r.e.finishReceiveOp(opID, ReceiveDiscarded, nil)
		r.bestEffortDeleteOwner(cli, mailboxID, env.TransferID)
		return
	}
	adm, accepted := r.e.admitReceive(receiveRequest{
		sourceApplication: clipMsg.SourceApplication,
		backend:           backendID,
		source:            DeviceID(env.SenderID),
		origin:            clipMsg.Origin,
		transfer:          clipMsg.ID,
		view:              clipMsg.Hash,
		total:             declaredTotal,
		parts:             recvParts,
	})
	if !accepted {
		r.e.finishReceiveOp(opID, ReceiveDiscarded, nil)
		r.bestEffortAck(cli, mailboxID, env.TransferID)
		return
	}

	snap, dir, rerr := extractRelaySnapshot(plainReader, clipMsg, r.e.recvDir(), &discarded)
	if discarded.Load() || errors.Is(rerr, errReceiveDiscarded) {
		adm.Fail(errReceiveDiscarded)
		if dir != "" {
			_ = os.RemoveAll(dir)
		}
		r.e.finishReceiveOp(opID, ReceiveDiscarded, nil)
		r.bestEffortDeleteOwner(cli, mailboxID, env.TransferID)
		return
	}
	if rerr == nil && snap.Hash() != clipMsg.Hash {
		rerr = newErr(CodeInvalidState, "relay clip hash mismatch")
	}
	if rerr != nil {
		adm.Fail(rerr)
		if dir != "" {
			_ = os.RemoveAll(dir)
		}
		r.e.finishReceiveOp(opID, ReceiveFailed, rerr)
		r.bestEffortDeleteOwner(cli, mailboxID, env.TransferID)
		return
	}

	adm.Done(snap, dir, true)
	r.e.finishReceiveOp(opID, ReceiveCommitted, nil)
	r.bestEffortAck(cli, mailboxID, env.TransferID)
}

func (r *relayCoordinator) waitChunkAvailable(ctx context.Context, asm *relayAssembly, cli *relayclient.Client, mailboxID, transferID string, totalChunks, idx int) error {
	for {
		asm.mu.Lock()
		ready := relaycrypto.HasBit(asm.available, idx)
		asm.mu.Unlock()
		if ready {
			return nil
		}
		t := time.NewTimer(relayChunkWaitPoll)
		select {
		case <-ctx.Done():
			t.Stop()
			return ctx.Err()
		case <-asm.chunkWake:
			t.Stop()
		case <-t.C:
			// Poll relay in case an SSE chunk_ready event was missed.
			listCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
			sums, err := cli.ListTransfers(listCtx, r.e.ident, mailboxID)
			cancel()
			if err == nil {
				found := false
				for _, sum := range sums {
					if sum.TransferID != transferID {
						continue
					}
					found = true
					if bm, berr := relaycrypto.DecodeBitmap(sum.AvailableBitmap, totalChunks); berr == nil {
						asm.mu.Lock()
						for j := 0; j < totalChunks; j++ {
							if relaycrypto.HasBit(bm, j) {
								relaycrypto.SetBit(asm.available, j)
							}
						}
						asm.mu.Unlock()
					}
				}
				if !found {
					return newErr(CodeNotFound, "relay transfer no longer available")
				}
			}
		}
	}
}

func (r *relayCoordinator) handleAssemblyAbort(opID OperationID, isDiscarded bool, cli *relayclient.Client, mailboxID, transferID string, err error) {
	if isDiscarded {
		r.e.finishReceiveOp(opID, ReceiveDiscarded, nil)
		r.bestEffortDeleteOwner(cli, mailboxID, transferID)
		return
	}
	r.e.finishReceiveOp(opID, ReceiveFailed, err)
}

func (r *relayCoordinator) bestEffortAck(cli *relayclient.Client, mailboxID, transferID string) {
	c, cancel := context.WithTimeout(context.Background(), relayCleanupTimeout)
	defer cancel()
	_ = cli.AckTransfer(c, r.e.ident, mailboxID, transferID)
}

func (r *relayCoordinator) bestEffortDeleteOwner(cli *relayclient.Client, mailboxID, transferID string) {
	c, cancel := context.WithTimeout(context.Background(), relayCleanupTimeout)
	defer cancel()
	_ = cli.DeleteTransferAsOwner(c, r.e.ident, mailboxID, transferID)
}

func extractRelaySnapshot(r io.Reader, m wire.Clip, inbox string, discarded *atomic.Bool) (s clip.Snapshot, dir string, err error) {
	s.SourceApplication = m.SourceApplication.Clone()
	defer func() {
		if err != nil && dir != "" {
			_ = os.RemoveAll(dir)
			dir = ""
		}
	}()
	for _, p := range m.Parts {
		if discarded != nil && discarded.Load() {
			return clip.Snapshot{}, dir, errReceiveDiscarded
		}
		switch p.Kind {
		case "fmt":
			buf := make([]byte, p.Size)
			if _, err = io.ReadFull(r, buf); err != nil {
				return
			}
			s.Parts = append(s.Parts, clip.Part{Format: p.Name, Data: buf})
		case "file":
			if dir == "" {
				if err = os.MkdirAll(inbox, 0o700); err != nil {
					return
				}
				if dir, err = os.MkdirTemp(inbox, "clip-"); err != nil {
					return
				}
			}
			name := wire.SafeName(p.Name)
			sub, merr := os.MkdirTemp(dir, "f-")
			if merr != nil {
				err = merr
				return
			}
			path := filepath.Join(sub, name)
			f, ferr := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
			if ferr != nil {
				err = ferr
				return
			}
			_, cerr := io.CopyN(f, r, p.Size)
			closeErr := f.Close()
			if cerr != nil {
				err = cerr
				return
			}
			if closeErr != nil {
				err = closeErr
				return
			}
			s.Files = append(s.Files, clip.File{Name: name, Path: path, Size: p.Size})
		default:
			return clip.Snapshot{}, dir, newErr(CodeInvalidArgument, "unknown part kind")
		}
	}
	return s, dir, nil
}

// authorizedPeerBackend returns the authorized peer backend ID for trusted senderID.
func (e *Engine) authorizedPeerBackend(senderID string) (PluginID, bool) {
	e.mu.Lock()
	procs := append([]*peerProcBackend(nil), e.peerProcs...)
	e.mu.Unlock()
	for _, pp := range procs {
		ok, err := e.allowedPeerOn(string(pp.id), senderID)
		if err == nil && ok && pp.syncEnabled(senderID) {
			return pp.id, true
		}
	}
	return "", false
}

// ---- outbound zero-wait relay fallback ----

type relayPlan struct {
	peerID        string
	peerPK        []byte
	route         store.RelayRoute
	effectiveMax  int64
	key           targetKey
	view          backendView
	clip          wire.Clip
}

// planRelayFallbacks finds authorized, sync-enabled offline peers of backend b
// that have a compatible relay route and fit within the relay size cap.
func (b *peerProcBackend) planRelayFallbacks(v backendView, liveSet map[string]bool) ([]relayPlan, *Error) {
	e := b.e
	if e.relay == nil {
		return nil, nil
	}
	cli, _, localMaxBytes, ok := e.relay.snapshotClient()
	if !ok {
		return nil, nil
	}
	ds, err := e.st.Devices()
	if err != nil {
		return nil, nil
	}
	files := hasFiles(v.set.c)
	var plans []relayPlan
	var sizeErr *Error
	for _, d := range ds {
		if d.Trust != store.TrustTrusted || liveSet[d.ID] {
			continue
		}
		if len(v.devices) > 0 && !slices.Contains(v.devices, DeviceID(d.ID)) {
			continue
		}
		bd, berr := e.st.InstanceBinding(d.ID, string(b.id))
		if berr != nil || bd.State != store.BindingAuthorized || !bd.SyncEnabled {
			continue
		}
		route, rerr := e.st.RelayRoute(d.ID)
		if rerr != nil || cli.ValidatePeerOrigin(route.RelayOrigin) != nil || route.MailboxID == "" || route.DeliveryToken == "" {
			continue
		}
		pv := v
		if !v.manual {
			effPol := applyBindingSendPolicy(v.backendPolicy, fromStoreBindingPolicy(bd.Policy))
			var peerSet sendSet
			if !files || effPol.Files {
				peerSet = buildSendSet(v.set.c, effPol)
			}
			if len(peerSet.parts) == 0 {
				if v.filtered != nil {
					*v.filtered = true
				}
				continue
			}
			pv.set = peerSet
		}
		effMax := localMaxBytes
		if route.MaxBytes > 0 && route.MaxBytes < effMax {
			effMax = route.MaxBytes
		}
		if pv.set.total > effMax {
			if v.manual {
				sizeErr = newErr(CodeLimitExceeded, "content exceeds relay size limit while peer is offline")
			}
			continue
		}
		plans = append(plans, relayPlan{
			peerID:       d.ID,
			peerPK:       append([]byte(nil), d.PublicKey...),
			route:        route,
			effectiveMax: effMax,
			key:          targetKey{b.id, TargetDevice, d.ID},
			view:         pv,
			clip:         b.wireClipOf(pv),
		})
	}
	if len(plans) == 0 && sizeErr != nil {
		return nil, sizeErr
	}
	return plans, nil
}

func (b *peerProcBackend) wireClipOf(v backendView) wire.Clip {
	c := wire.Clip{
		SourceApplication: v.set.c.source.Clone(),
		T:                 "clip",
		ID:                v.id,
		Origin:            b.e.ident.DeviceID(),
		TS:                max(v.tsMs, 0),
		Hash:              v.set.hash,
	}
	for _, p := range v.set.parts {
		pm := wire.PartMeta{Size: p.desc.Size}
		if p.desc.Kind == PartFile {
			pm.Kind, pm.Name = "file", p.desc.Name
		} else {
			pm.Kind, pm.Name = "fmt", p.desc.MIME
		}
		c.Parts = append(c.Parts, pm)
	}
	return c
}

func (b *peerProcBackend) wakePeer(peerID string) {
	if bridge, ok := b.pb.(plugin.PeerRelayBridge); ok {
		bridge.WakePeer(peerID)
	}
}

func (b *peerProcBackend) sendViaRelay(ctx context.Context, plan relayPlan, h backendHost) error {
	e := b.e
	cli, _, _, ok := e.relay.snapshotClient()
	if !ok {
		return newErr(CodeUnavailable, "relay is unavailable")
	}
	if err := ctx.Err(); err != nil {
		return ctxErr(err)
	}
	// Re-verify authorization, sync switches and limits before opening parts.
	allowed, err := e.allowedPeerOn(string(b.id), plan.peerID)
	if err != nil || !allowed || !b.syncEnabled(plan.peerID) {
		return newErr(CodePermissionDenied, "peer is no longer authorized")
	}
	e.mu.Lock()
	rerr := e.needRunningLocked()
	enabled, relayEnabled, lim := e.settings.Value.Sync.Enabled, e.settings.Value.Sync.Relay.Enabled, e.settings.Value.Limits
	e.mu.Unlock()
	if rerr != nil {
		return rerr
	}
	if !enabled || !relayEnabled {
		return newErr(CodeSyncDisabled, "sync is paused")
	}
	if lerr := checkLimits(plan.view.set, lim); lerr != nil {
		return lerr
	}

	// Open all parts upfront so source_changed is detected before upload starts.
	hs := make([]io.ReadCloser, len(plan.view.set.parts))
	closeAll := func() {
		for _, rc := range hs {
			if rc != nil {
				_ = rc.Close()
			}
		}
	}
	defer closeAll()
	for i, p := range plan.view.set.parts {
		if p.data != nil {
			hs[i] = io.NopCloser(bytes.NewReader(p.data))
			continue
		}
		octx, cancel := context.WithTimeout(ctx, 30*time.Second)
		rc, oerr := e.openPart(octx, p)
		cancel()
		if oerr != nil {
			return oerr
		}
		hs[i] = rc
	}

	hdrBytes, err := relaycrypto.EncodePlaintextHeader(plan.clip)
	if err != nil {
		return err
	}
	plainTotal := int64(len(hdrBytes)) + plan.view.set.total
	chunkSize, totalChunks, err := relaycrypto.ChunkParams(plainTotal)
	if err != nil {
		return newErr(CodeLimitExceeded, "content too large for relay chunking")
	}

	cek := make([]byte, relaycrypto.CEKSize)
	if _, err := rand.Read(cek); err != nil {
		return newErr(CodeInternal, "random key generation failed")
	}
	cc, err := relaycrypto.NewChunkCipher(cek, plan.clip.ID, e.ident.DeviceID(), plan.peerID, totalChunks)
	if err != nil {
		return err
	}

	// Build unified sequential plaintext reader: header || part[0] || ...
	readers := make([]io.Reader, 0, 1+len(hs))
	readers = append(readers, bytes.NewReader(hdrBytes))
	for i, rc := range hs {
		readers = append(readers, io.LimitReader(rc, plan.view.set.parts[i].desc.Size))
	}
	seq := io.MultiReader(readers...)

	// Encrypt chunks sequentially to compute exact chunk_hashes for the signed envelope.
	// Single-chunk transfers stay in memory (<= 2 MiB); multi-chunk transfers are
	// bounded by effectiveMax (default 25 MiB).
	cts := make([][]byte, totalChunks)
	hashes := make([][32]byte, totalChunks)
	var totalCT int64
	buf := make([]byte, chunkSize)
	for i := 0; i < totalChunks; i++ {
		if err := ctx.Err(); err != nil {
			return ctxErr(err)
		}
		want := chunkSize
		if rem := plainTotal - int64(i)*int64(chunkSize); rem < int64(want) {
			want = int(rem)
		}
		if _, err := io.ReadFull(seq, buf[:want]); err != nil {
			return err
		}
		ct, hsum, err := cc.SealChunk(i, buf[:want])
		if err != nil {
			return err
		}
		cts[i] = ct
		hashes[i] = hsum
		totalCT += int64(len(ct))
	}
	if totalCT > plan.effectiveMax+int64(wire.MaxControl)+int64(totalChunks)*relaycrypto.GCMTagSize {
		return newErr(CodeLimitExceeded, "encrypted payload exceeds relay limit")
	}

	now := time.Now()
	env, err := relaycrypto.SealEnvelope(e.ident, relaycrypto.SealEnvelopeInput{
		RelayOrigin:          cli.Origin(),
		TransferID:           plan.clip.ID,
		RecipientPK:          plan.peerPK,
		CreatedMs:            now.UnixMilli(),
		ExpiresMs:            now.Add(relaycrypto.DefaultTTL).UnixMilli(),
		ChunkSize:            chunkSize,
		TotalChunks:          totalChunks,
		TotalCiphertextBytes: totalCT,
		ChunkHashes:          hashes,
		CEK:                  cek,
	})
	if err != nil {
		return err
	}

	// Fast path: 1-RTT single-chunk upload.
	if totalChunks == 1 {
		if err := cli.UploadSingle(ctx, plan.route.MailboxID, plan.route.DeliveryToken, env, cts[0]); err != nil {
			return err
		}
		h.ReportProgress(plan.key, "", TransferProgress{BytesSent: plan.view.set.total, TotalBytes: plan.view.set.total, PartCount: len(plan.view.set.parts)})
		return nil
	}

	// Multi-chunk path: announce envelope then upload chunks with bounded concurrency.
	if _, err := cli.CreateTransfer(ctx, plan.route.MailboxID, plan.route.DeliveryToken, env); err != nil {
		return err
	}
	upCtx, upCancel := context.WithCancel(ctx)
	defer upCancel()

	type chunkTask struct {
		idx int
		ct  []byte
	}
	tasks := make(chan chunkTask, totalChunks)
	for i := 0; i < totalChunks; i++ {
		tasks <- chunkTask{idx: i, ct: cts[i]}
	}
	close(tasks)

	var (
		wg       sync.WaitGroup
		errMu    sync.Mutex
		firstErr error
		sentCT   atomic.Int64
	)
	workers := min(relayUploadConcurrency, totalChunks)
	for w := 0; w < workers; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for t := range tasks {
				if upCtx.Err() != nil {
					return
				}
				if err := cli.UploadChunk(upCtx, plan.route.MailboxID, plan.route.DeliveryToken, plan.clip.ID, t.idx, t.ct); err != nil {
					errMu.Lock()
					if firstErr == nil {
						firstErr = err
					}
					errMu.Unlock()
					upCancel()
					return
				}
				doneCT := sentCT.Add(int64(len(t.ct)))
				approxPlain := min((doneCT*plan.view.set.total)/max(totalCT, 1), plan.view.set.total)
				h.ReportProgress(plan.key, "", TransferProgress{
					BytesSent:  approxPlain,
					TotalBytes: plan.view.set.total,
					PartCount:  len(plan.view.set.parts),
				})
			}
		}()
	}
	wg.Wait()
	if firstErr != nil {
		delCtx, cancel := context.WithTimeout(context.Background(), relayCleanupTimeout)
		_ = cli.DeleteTransferAsSender(delCtx, plan.route.MailboxID, plan.route.DeliveryToken, plan.clip.ID)
		cancel()
		return firstErr
	}
	return nil
}

// encodeJSONForTest keeps json import used cleanly if needed in future extensions.
var _ = json.Marshal
