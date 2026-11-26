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
	handoffMinRemainingBytes  = 2 << 20 // 2 MiB (contract §5.1)
	handoffMinRemainingChunks = 2
	maxHandoffSwitches        = 3
	handoffClipAckTimeout     = 30 * time.Second
	maxCompletedAssemblies    = 256
)

// relayCoordinator manages E2EE HTTP relay client lifecycle, per-peer delivery
// token registration, SSE mailbox subscription, inbound chunked assembly
// (shared across HTTP and Phase 2 P2P handoff), and outbound zero-wait relay
// uploads with Level 3 P2P handoff settlement and reverse fallback (0035).
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
	completed  map[string]int64 // asmKey -> unix ms committed
	peerUpWake map[string]map[chan struct{}]struct{}
}

type relayAssembly struct {
	r          *relayCoordinator
	transferID string
	senderID   string
	backendID  PluginID
	ve         relaycrypto.VerifiedEnvelope
	cc         *relaycrypto.ChunkCipher

	opID      OperationID
	cancel    context.CancelFunc
	discarded atomic.Bool

	mu           sync.Mutex
	stageFile    *os.File // nil when TotalChunks == 1
	stagePath    string
	memSingle    []byte // used when TotalChunks == 1
	have         []byte // verified & written chunks
	haveCount    int
	plainBytes   int64
	available    []byte // chunks known available on HTTP relay
	p2pWanted    []byte // chunks currently requested over P2P
	httpInflight map[int]bool
	p2pUsed      bool
	closed       bool
	chunkWake    chan struct{}
}

type relaySendResult struct {
	state        TargetState
	receiptStage ReceiptStage
	route        TransportRoute
}

func asmKey(senderID, transferID string) string { return senderID + ":" + transferID }

func newRelayCoordinator(e *Engine) *relayCoordinator {
	return &relayCoordinator{
		e:          e,
		registered: map[string]bool{},
		assemblies: map[string]*relayAssembly{},
		completed:  map[string]int64{},
		peerUpWake: map[string]map[chan struct{}]struct{}{},
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
		var asm *relayAssembly
		for _, a := range r.assemblies {
			if a.transferID == ev.TransferID {
				asm = a
				break
			}
		}
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
		asm.wake()
	}
}

func (a *relayAssembly) wake() {
	select {
	case a.chunkWake <- struct{}{}:
	default:
	}
}

func (r *relayCoordinator) handoffBridge(backendID PluginID) plugin.PeerRelayHandoffBridge {
	r.e.mu.Lock()
	procs := append([]*peerProcBackend(nil), r.e.peerProcs...)
	r.e.mu.Unlock()
	for _, pp := range procs {
		if backendID != "" && pp.id != backendID {
			continue
		}
		if hb, ok := pp.pb.(plugin.PeerRelayHandoffBridge); ok {
			return hb
		}
	}
	return nil
}

func (r *relayCoordinator) sendClipAck(backendID PluginID, peerID, transferID, status string) {
	if hb := r.handoffBridge(backendID); hb != nil {
		hb.SendRelayClipAck(peerID, transferID, status)
	}
}

func (r *relayCoordinator) watchPeerUp(peerID string) (<-chan struct{}, func()) {
	ch := make(chan struct{}, 1)
	r.mu.Lock()
	set := r.peerUpWake[peerID]
	if set == nil {
		set = map[chan struct{}]struct{}{}
		r.peerUpWake[peerID] = set
	}
	set[ch] = struct{}{}
	r.mu.Unlock()
	return ch, func() {
		r.mu.Lock()
		if s := r.peerUpWake[peerID]; s != nil {
			delete(s, ch)
			if len(s) == 0 {
				delete(r.peerUpWake, peerID)
			}
		}
		r.mu.Unlock()
	}
}

func (r *relayCoordinator) onPeerConnected(peerID string, handoffCap, _ bool) {
	if !handoffCap || peerID == "" {
		return
	}
	r.mu.Lock()
	for ch := range r.peerUpWake[peerID] {
		select {
		case ch <- struct{}{}:
		default:
		}
	}
	r.mu.Unlock()
}

func (r *relayCoordinator) onPeerDisconnected(peerID string) {
	if peerID == "" {
		return
	}
	r.mu.Lock()
	var matching []*relayAssembly
	for _, asm := range r.assemblies {
		if asm.senderID == peerID {
			matching = append(matching, asm)
		}
	}
	r.mu.Unlock()
	for _, asm := range matching {
		asm.mu.Lock()
		for i := range asm.p2pWanted {
			asm.p2pWanted[i] = 0
		}
		asm.mu.Unlock()
		asm.wake()
	}
}

func (r *relayCoordinator) isCompleted(senderID, transferID string) bool {
	key := asmKey(senderID, transferID)
	r.mu.Lock()
	_, ok := r.completed[key]
	r.mu.Unlock()
	return ok
}

func (r *relayCoordinator) markCompleted(senderID, transferID string) {
	key := asmKey(senderID, transferID)
	now := time.Now().UnixMilli()
	r.mu.Lock()
	r.completed[key] = now
	if len(r.completed) > maxCompletedAssemblies {
		var oldestKey string
		var oldestTs int64
		for k, ts := range r.completed {
			if oldestKey == "" || ts < oldestTs {
				oldestKey, oldestTs = k, ts
			}
		}
		if oldestKey != "" {
			delete(r.completed, oldestKey)
		}
	}
	r.mu.Unlock()
}

// onHandoffOffer handles a Level 3 relay_handoff_offer received over an
// authenticated P2P session (contract §5.2).
func (r *relayCoordinator) onHandoffOffer(peerID string, offer plugin.RelayHandoffOffer) plugin.RelayHandoffAck {
	reject := func(reason string) plugin.RelayHandoffAck {
		return plugin.RelayHandoffAck{TransferID: offer.TransferID, Accept: false, Reason: reason}
	}
	cli, mailboxID, _, ok := r.snapshotClient()
	if !ok {
		return reject("unavailable")
	}
	var env relaycrypto.Envelope
	if err := json.Unmarshal(offer.EnvelopeJSON, &env); err != nil {
		return reject("invalid_argument")
	}
	if env.TransferID != offer.TransferID || env.SenderID != peerID {
		return reject("invalid_argument")
	}
	if r.isCompleted(peerID, env.TransferID) {
		allOnes, _ := relaycrypto.NewBitmap(env.TotalChunks)
		allZeros, _ := relaycrypto.NewBitmap(env.TotalChunks)
		for i := 0; i < env.TotalChunks; i++ {
			relaycrypto.SetBit(allOnes, i)
		}
		if bid, allowed := r.e.authorizedPeerBackend(peerID); allowed {
			r.sendClipAck(bid, peerID, env.TransferID, wire.ClipAckReceived)
		}
		return plugin.RelayHandoffAck{
			TransferID:    env.TransferID,
			Accept:        true,
			HaveBitmap:    relaycrypto.EncodeBitmap(allOnes),
			WantP2PBitmap: relaycrypto.EncodeBitmap(allZeros),
		}
	}
	relayUploadedBM, err := relaycrypto.DecodeBitmap(offer.RelayUploadedBitmap, env.TotalChunks)
	if err != nil {
		relayUploadedBM, _ = relaycrypto.NewBitmap(env.TotalChunks)
	}
	r.e.mu.Lock()
	baseCtx := r.e.runCtx
	r.e.mu.Unlock()
	if baseCtx == nil {
		return reject("unavailable")
	}
	asm, alreadyDone, rejReason := r.admitOrGetAssembly(baseCtx, cli, mailboxID, cli.Origin(), env, relayUploadedBM, true)
	if alreadyDone {
		allOnes, _ := relaycrypto.NewBitmap(env.TotalChunks)
		allZeros, _ := relaycrypto.NewBitmap(env.TotalChunks)
		for i := 0; i < env.TotalChunks; i++ {
			relaycrypto.SetBit(allOnes, i)
		}
		if bid, allowed := r.e.authorizedPeerBackend(peerID); allowed {
			r.sendClipAck(bid, peerID, env.TransferID, wire.ClipAckReceived)
		}
		return plugin.RelayHandoffAck{
			TransferID:    env.TransferID,
			Accept:        true,
			HaveBitmap:    relaycrypto.EncodeBitmap(allOnes),
			WantP2PBitmap: relaycrypto.EncodeBitmap(allZeros),
		}
	}
	if asm == nil {
		if rejReason == "" {
			rejReason = "policy"
		}
		return reject(rejReason)
	}

	asm.mu.Lock()
	if asm.discarded.Load() {
		asm.mu.Unlock()
		return reject("discarded")
	}
	asm.p2pUsed = true
	for i := 0; i < env.TotalChunks; i++ {
		if relaycrypto.HasBit(relayUploadedBM, i) {
			relaycrypto.SetBit(asm.available, i)
		}
	}
	haveCopy := append([]byte(nil), asm.have...)
	wantBM, _ := relaycrypto.NewBitmap(env.TotalChunks)
	for i := 0; i < env.TotalChunks; i++ {
		if relaycrypto.HasBit(asm.have, i) {
			continue
		}
		// Any chunk not yet on relay MUST be requested via P2P; chunks already on
		// relay that aren't currently mid-HTTP-download also shift to fast P2P.
		if !relaycrypto.HasBit(relayUploadedBM, i) || !asm.httpInflight[i] {
			relaycrypto.SetBit(wantBM, i)
		}
	}
	asm.p2pWanted = append([]byte(nil), wantBM...)
	asm.mu.Unlock()
	asm.wake()

	return plugin.RelayHandoffAck{
		TransferID:    env.TransferID,
		Accept:        true,
		HaveBitmap:    relaycrypto.EncodeBitmap(haveCopy),
		WantP2PBitmap: relaycrypto.EncodeBitmap(wantBM),
	}
}

// onHandoffChunk ingests one encrypted chunk pushed over P2P into the shared
// TransferAssembly (idempotent with HTTP downloads).
func (r *relayCoordinator) onHandoffChunk(peerID, transferID string, index int, ciphertext []byte) {
	r.mu.Lock()
	asm := r.assemblies[asmKey(peerID, transferID)]
	r.mu.Unlock()
	if asm == nil {
		return
	}
	_, _, _ = asm.writeChunk(index, ciphertext)
}

func (a *relayAssembly) writeChunk(idx int, ct []byte) (accepted bool, allDone bool, err error) {
	if a.discarded.Load() {
		return false, false, errReceiveDiscarded
	}
	env := a.ve.Envelope
	if idx < 0 || idx >= env.TotalChunks {
		return false, false, newErr(CodeInvalidArgument, "chunk index out of range")
	}
	a.mu.Lock()
	if a.closed || relaycrypto.HasBit(a.have, idx) {
		done := a.haveCount == env.TotalChunks
		a.mu.Unlock()
		return false, done, nil
	}
	a.mu.Unlock()

	pt, oerr := a.cc.OpenChunk(idx, ct, a.ve.ChunkHashes[idx])
	if oerr != nil {
		return false, false, oerr
	}
	if idx < env.TotalChunks-1 && len(pt) != env.ChunkSize {
		return false, false, newErr(CodeInvalidArgument, "unexpected intermediate chunk plaintext size")
	}
	if idx == env.TotalChunks-1 && (len(pt) == 0 || len(pt) > env.ChunkSize) {
		return false, false, newErr(CodeInvalidArgument, "unexpected final chunk plaintext size")
	}

	a.mu.Lock()
	if a.closed || a.discarded.Load() || relaycrypto.HasBit(a.have, idx) {
		done := a.haveCount == env.TotalChunks
		a.mu.Unlock()
		return false, done, nil
	}
	if a.stageFile != nil {
		off := int64(idx) * int64(env.ChunkSize)
		if _, werr := a.stageFile.WriteAt(pt, off); werr != nil {
			a.mu.Unlock()
			return false, false, werr
		}
	} else {
		a.memSingle = append([]byte(nil), pt...)
	}
	relaycrypto.SetBit(a.have, idx)
	relaycrypto.ClearBit(a.p2pWanted, idx)
	a.haveCount++
	a.plainBytes += int64(len(pt))
	plainBytes := a.plainBytes
	done := a.haveCount == env.TotalChunks
	a.mu.Unlock()

	a.r.e.updateReceiveProgress(a.opID, plainBytes, idx)
	a.wake()
	return true, done, nil
}

func (r *relayCoordinator) admitAssembly(parentCtx context.Context, cli *relayclient.Client, mailboxID, origin string, env relaycrypto.Envelope, initialBM []byte) {
	_, alreadyDone, _ := r.admitOrGetAssembly(parentCtx, cli, mailboxID, origin, env, initialBM, false)
	if alreadyDone {
		r.bestEffortAck(cli, mailboxID, env.TransferID)
	}
}

func (r *relayCoordinator) admitOrGetAssembly(
	parentCtx context.Context,
	cli *relayclient.Client,
	mailboxID, origin string,
	env relaycrypto.Envelope,
	initialBM []byte,
	fromP2P bool,
) (asm *relayAssembly, alreadyDone bool, rejectReason string) {
	if r.isCompleted(env.SenderID, env.TransferID) {
		return nil, true, ""
	}
	// Step 1: Validate sender trust and authorization before any decryption or disk I/O.
	d, err := r.e.st.Device(env.SenderID)
	if err != nil || d.Trust != store.TrustTrusted {
		return nil, false, "permission_denied"
	}
	backendID, allowed := r.e.authorizedPeerBackend(env.SenderID)
	if !allowed {
		return nil, false, "policy"
	}
	ve, err := relaycrypto.VerifyEnvelope(origin, env, r.e.ident.DeviceID(), d.PublicKey, time.Now())
	if err != nil {
		return nil, false, "invalid_argument"
	}
	r.e.mu.Lock()
	lim := r.e.settings.Value.Limits
	syncOn := r.e.settings.Value.Sync.Enabled
	r.e.mu.Unlock()
	if !syncOn {
		return nil, false, "policy"
	}
	// Ciphertext includes GCM tag per chunk; reject oversized envelopes up front.
	maxAllowedCT := lim.MaxTotalBytes + int64(wire.MaxControl) + int64(env.TotalChunks)*relaycrypto.GCMTagSize
	if env.TotalCiphertextBytes > maxAllowedCT {
		r.bestEffortDeleteOwner(cli, mailboxID, env.TransferID)
		return nil, false, "limit_exceeded"
	}
	cek, err := relaycrypto.OpenCEK(r.e.ident, origin, ve)
	if err != nil {
		return nil, false, "invalid_argument"
	}
	cc, err := relaycrypto.NewChunkCipher(cek, env.TransferID, env.SenderID, env.RecipientID, env.TotalChunks)
	if err != nil {
		return nil, false, "invalid_argument"
	}

	key := asmKey(env.SenderID, env.TransferID)
	r.mu.Lock()
	if !r.active {
		r.mu.Unlock()
		return nil, false, "unavailable"
	}
	if _, done := r.completed[key]; done {
		r.mu.Unlock()
		return nil, true, ""
	}
	if existing := r.assemblies[key]; existing != nil {
		existing.mu.Lock()
		for i := 0; i < env.TotalChunks; i++ {
			if relaycrypto.HasBit(initialBM, i) {
				relaycrypto.SetBit(existing.available, i)
			}
		}
		if fromP2P {
			existing.p2pUsed = true
		}
		existing.mu.Unlock()
		existing.wake()
		r.mu.Unlock()
		return existing, false, ""
	}
	if len(r.assemblies) >= maxActiveRelayAssemblies {
		r.mu.Unlock()
		return nil, false, "resource_exhausted"
	}

	var stageFile *os.File
	var stagePath string
	if env.TotalChunks > 1 {
		if err := os.MkdirAll(r.e.recvDir(), 0o700); err != nil {
			r.mu.Unlock()
			return nil, false, "unavailable"
		}
		f, ferr := os.CreateTemp(r.e.recvDir(), "relay-*")
		if ferr != nil {
			r.mu.Unlock()
			return nil, false, "unavailable"
		}
		stageFile = f
		stagePath = f.Name()
	}

	haveBM, _ := relaycrypto.NewBitmap(env.TotalChunks)
	wantBM, _ := relaycrypto.NewBitmap(env.TotalChunks)
	availBM := append([]byte(nil), initialBM...)
	if len(availBM) < len(haveBM) {
		availBM, _ = relaycrypto.NewBitmap(env.TotalChunks)
	}

	asmCtx, asmCancel := context.WithCancel(parentCtx)
	if exp := time.UnixMilli(env.ExpiresMs); !exp.IsZero() {
		var dlCancel context.CancelFunc
		asmCtx, dlCancel = context.WithDeadline(asmCtx, exp)
		origCancel := asmCancel
		asmCancel = func() {
			dlCancel()
			origCancel()
		}
	}

	asm = &relayAssembly{
		r:            r,
		transferID:   env.TransferID,
		senderID:     env.SenderID,
		backendID:    backendID,
		ve:           ve,
		cc:           cc,
		cancel:       asmCancel,
		stageFile:    stageFile,
		stagePath:    stagePath,
		have:         haveBM,
		available:    availBM,
		p2pWanted:    wantBM,
		httpInflight: map[int]bool{},
		p2pUsed:      fromP2P,
		chunkWake:    make(chan struct{}, 1),
	}
	r.assemblies[key] = asm
	r.wg.Add(1)
	r.mu.Unlock()

	estPlain := max(env.TotalCiphertextBytes-int64(env.TotalChunks)*relaycrypto.GCMTagSize, 0)
	asm.opID = r.e.startReceiveOp(ReceiveOperation{
		BackendID:   backendID,
		PeerID:      DeviceID(env.SenderID),
		State:       ReceiveReceiving,
		ContentKind: "unknown",
		Progress: ReceiveProgress{
			TotalBytes: estPlain,
			PartCount:  env.TotalChunks,
		},
	}, func() {
		asm.discarded.Store(true)
		asm.mu.Lock()
		usedP2P := asm.p2pUsed
		asm.mu.Unlock()
		if usedP2P {
			r.sendClipAck(backendID, env.SenderID, env.TransferID, wire.ClipAckDiscarded)
		}
		asmCancel()
	})

	go func() {
		defer r.wg.Done()
		defer func() {
			asmCancel()
			asm.mu.Lock()
			asm.closed = true
			sf := asm.stageFile
			sp := asm.stagePath
			asm.stageFile = nil
			asm.mu.Unlock()
			if sf != nil {
				_ = sf.Close()
			}
			if sp != "" {
				_ = os.Remove(sp)
			}
			r.mu.Lock()
			if r.assemblies[key] == asm {
				delete(r.assemblies, key)
			}
			r.mu.Unlock()
		}()
		r.runAssembly(asmCtx, asm, cli, mailboxID)
	}()
	return asm, false, ""
}

func (r *relayCoordinator) runAssembly(
	ctx context.Context,
	asm *relayAssembly,
	cli *relayclient.Client,
	mailboxID string,
) {
	env := asm.ve.Envelope
	maxChunkCT := int64(env.ChunkSize) + relaycrypto.GCMTagSize

	for {
		if asm.discarded.Load() {
			r.handleAssemblyAbort(asm, cli, mailboxID, errReceiveDiscarded)
			return
		}
		asm.mu.Lock()
		allDone := asm.haveCount == env.TotalChunks
		nextHTTP := -1
		if !allDone {
			for i := 0; i < env.TotalChunks; i++ {
				if !relaycrypto.HasBit(asm.have, i) &&
					relaycrypto.HasBit(asm.available, i) &&
					!relaycrypto.HasBit(asm.p2pWanted, i) &&
					!asm.httpInflight[i] {
					nextHTTP = i
					asm.httpInflight[i] = true
					break
				}
			}
		}
		p2pActive := asm.p2pUsed
		asm.mu.Unlock()

		if allDone {
			break
		}
		if nextHTTP >= 0 {
			ct, derr := cli.DownloadChunk(ctx, r.e.ident, mailboxID, env.TransferID, nextHTTP, maxChunkCT)
			asm.mu.Lock()
			delete(asm.httpInflight, nextHTTP)
			usedP2PNow := asm.p2pUsed
			alreadyHave := relaycrypto.HasBit(asm.have, nextHTTP)
			asm.mu.Unlock()
			if derr != nil {
				if asm.discarded.Load() || ctx.Err() != nil {
					r.handleAssemblyAbort(asm, cli, mailboxID, derr)
					return
				}
				// If P2P handoff delivered it concurrently or is active, continue loop.
				if alreadyHave || usedP2PNow {
					continue
				}
				r.handleAssemblyAbort(asm, cli, mailboxID, derr)
				return
			}
			if _, _, werr := asm.writeChunk(nextHTTP, ct); werr != nil {
				r.handleAssemblyAbort(asm, cli, mailboxID, werr)
				return
			}
			continue
		}

		// Wait for either an SSE chunk_ready, a P2P relay_chunk_data, or poll tick.
		t := time.NewTimer(relayChunkWaitPoll)
		select {
		case <-ctx.Done():
			t.Stop()
			r.handleAssemblyAbort(asm, cli, mailboxID, ctx.Err())
			return
		case <-asm.chunkWake:
			t.Stop()
		case <-t.C:
			listCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
			sums, err := cli.ListTransfers(listCtx, r.e.ident, mailboxID)
			cancel()
			if err == nil {
				found := false
				for _, sum := range sums {
					if sum.TransferID != env.TransferID {
						continue
					}
					found = true
					if bm, berr := relaycrypto.DecodeBitmap(sum.AvailableBitmap, env.TotalChunks); berr == nil {
						asm.mu.Lock()
						for j := 0; j < env.TotalChunks; j++ {
							if relaycrypto.HasBit(bm, j) {
								relaycrypto.SetBit(asm.available, j)
							}
						}
						asm.mu.Unlock()
					}
				}
				if !found && !p2pActive {
					r.handleAssemblyAbort(asm, cli, mailboxID, newErr(CodeNotFound, "relay transfer no longer available"))
					return
				}
			}
		}
	}

	if asm.discarded.Load() {
		r.handleAssemblyAbort(asm, cli, mailboxID, errReceiveDiscarded)
		return
	}

	asm.mu.Lock()
	asm.closed = true
	p2pUsed := asm.p2pUsed
	plainTotal := asm.plainBytes
	sf := asm.stageFile
	single := asm.memSingle
	asm.mu.Unlock()

	var plainReader io.Reader
	if sf != nil {
		if _, serr := sf.Seek(0, io.SeekStart); serr != nil {
			r.e.finishReceiveOp(asm.opID, ReceiveFailed, serr)
			if p2pUsed {
				r.sendClipAck(asm.backendID, env.SenderID, env.TransferID, wire.ClipAckFailed)
			}
			return
		}
		plainReader = io.LimitReader(sf, plainTotal)
	} else {
		plainReader = bytes.NewReader(single)
	}

	clipMsg, herr := relaycrypto.ReadPlaintextHeader(plainReader)
	if herr != nil || clipMsg.ID != env.TransferID || clipMsg.Origin != env.SenderID {
		r.e.finishReceiveOp(asm.opID, ReceiveFailed, newErr(CodeInvalidArgument, "malformed relay clip header"))
		r.bestEffortDeleteOwner(cli, mailboxID, env.TransferID)
		if p2pUsed {
			r.sendClipAck(asm.backendID, env.SenderID, env.TransferID, wire.ClipAckFailed)
		}
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
		r.e.finishReceiveOp(asm.opID, ReceiveDiscarded, nil)
		r.bestEffortAck(cli, mailboxID, env.TransferID)
		if p2pUsed {
			r.sendClipAck(asm.backendID, env.SenderID, env.TransferID, wire.ClipAckDiscarded)
		}
		return
	}
	// Re-check peer authorization immediately before admission.
	if _, ok := r.e.authorizedPeerBackend(env.SenderID); !ok {
		r.e.finishReceiveOp(asm.opID, ReceiveDiscarded, nil)
		r.bestEffortDeleteOwner(cli, mailboxID, env.TransferID)
		if p2pUsed {
			r.sendClipAck(asm.backendID, env.SenderID, env.TransferID, wire.ClipAckDiscarded)
		}
		return
	}
	adm, accepted := r.e.admitReceive(receiveRequest{
		sourceApplication: clipMsg.SourceApplication,
		backend:           asm.backendID,
		source:            DeviceID(env.SenderID),
		origin:            clipMsg.Origin,
		transfer:          clipMsg.ID,
		view:              clipMsg.Hash,
		total:             declaredTotal,
		parts:             recvParts,
	})
	if !accepted {
		r.e.finishReceiveOp(asm.opID, ReceiveDiscarded, nil)
		r.bestEffortAck(cli, mailboxID, env.TransferID)
		if p2pUsed {
			r.sendClipAck(asm.backendID, env.SenderID, env.TransferID, wire.ClipAckDiscarded)
		}
		return
	}

	snap, dir, rerr := extractRelaySnapshot(plainReader, clipMsg, r.e.recvDir(), &asm.discarded)
	if asm.discarded.Load() || errors.Is(rerr, errReceiveDiscarded) {
		adm.Fail(errReceiveDiscarded)
		if dir != "" {
			_ = os.RemoveAll(dir)
		}
		r.e.finishReceiveOp(asm.opID, ReceiveDiscarded, nil)
		r.bestEffortDeleteOwner(cli, mailboxID, env.TransferID)
		if p2pUsed {
			r.sendClipAck(asm.backendID, env.SenderID, env.TransferID, wire.ClipAckDiscarded)
		}
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
		r.e.finishReceiveOp(asm.opID, ReceiveFailed, rerr)
		r.bestEffortDeleteOwner(cli, mailboxID, env.TransferID)
		if p2pUsed {
			r.sendClipAck(asm.backendID, env.SenderID, env.TransferID, wire.ClipAckFailed)
		}
		return
	}

	adm.Done(snap, dir, true)
	r.markCompleted(env.SenderID, env.TransferID)
	r.e.finishReceiveOp(asm.opID, ReceiveCommitted, nil)
	r.bestEffortAck(cli, mailboxID, env.TransferID)
	if p2pUsed {
		r.sendClipAck(asm.backendID, env.SenderID, env.TransferID, wire.ClipAckReceived)
	}
}

func (r *relayCoordinator) handleAssemblyAbort(asm *relayAssembly, cli *relayclient.Client, mailboxID string, err error) {
	asm.mu.Lock()
	usedP2P := asm.p2pUsed
	asm.mu.Unlock()
	if asm.discarded.Load() {
		r.e.finishReceiveOp(asm.opID, ReceiveDiscarded, nil)
		r.bestEffortDeleteOwner(cli, mailboxID, asm.transferID)
		if usedP2P {
			r.sendClipAck(asm.backendID, asm.senderID, asm.transferID, wire.ClipAckDiscarded)
		}
		return
	}
	r.e.finishReceiveOp(asm.opID, ReceiveFailed, err)
	if usedP2P {
		r.sendClipAck(asm.backendID, asm.senderID, asm.transferID, wire.ClipAckFailed)
	}
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

func (b *peerProcBackend) sendViaRelay(ctx context.Context, plan relayPlan, h backendHost) (relaySendResult, error) {
	e := b.e
	cli, _, _, ok := e.relay.snapshotClient()
	if !ok {
		return relaySendResult{}, newErr(CodeUnavailable, "relay is unavailable")
	}
	if err := ctx.Err(); err != nil {
		return relaySendResult{}, ctxErr(err)
	}
	// Re-verify authorization, sync switches and limits before opening parts.
	allowed, err := e.allowedPeerOn(string(b.id), plan.peerID)
	if err != nil || !allowed || !b.syncEnabled(plan.peerID) {
		return relaySendResult{}, newErr(CodePermissionDenied, "peer is no longer authorized")
	}
	e.mu.Lock()
	rerr := e.needRunningLocked()
	enabled, relayEnabled, lim := e.settings.Value.Sync.Enabled, e.settings.Value.Sync.Relay.Enabled, e.settings.Value.Limits
	e.mu.Unlock()
	if rerr != nil {
		return relaySendResult{}, rerr
	}
	if !enabled || !relayEnabled {
		return relaySendResult{}, newErr(CodeSyncDisabled, "sync is paused")
	}
	if lerr := checkLimits(plan.view.set, lim); lerr != nil {
		return relaySendResult{}, lerr
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
			return relaySendResult{}, oerr
		}
		hs[i] = rc
	}

	hdrBytes, err := relaycrypto.EncodePlaintextHeader(plan.clip)
	if err != nil {
		return relaySendResult{}, err
	}
	plainTotal := int64(len(hdrBytes)) + plan.view.set.total
	chunkSize, totalChunks, err := relaycrypto.ChunkParams(plainTotal)
	if err != nil {
		return relaySendResult{}, newErr(CodeLimitExceeded, "content too large for relay chunking")
	}

	cek := make([]byte, relaycrypto.CEKSize)
	if _, err := rand.Read(cek); err != nil {
		return relaySendResult{}, newErr(CodeInternal, "random key generation failed")
	}
	cc, err := relaycrypto.NewChunkCipher(cek, plan.clip.ID, e.ident.DeviceID(), plan.peerID, totalChunks)
	if err != nil {
		return relaySendResult{}, err
	}

	// Build unified sequential plaintext reader: header || part[0] || ...
	readers := make([]io.Reader, 0, 1+len(hs))
	readers = append(readers, bytes.NewReader(hdrBytes))
	for i, rc := range hs {
		readers = append(readers, io.LimitReader(rc, plan.view.set.parts[i].desc.Size))
	}
	seq := io.MultiReader(readers...)

	// Encrypt chunks sequentially to compute exact chunk_hashes for the signed envelope.
	cts := make([][]byte, totalChunks)
	hashes := make([][32]byte, totalChunks)
	var totalCT int64
	buf := make([]byte, chunkSize)
	for i := 0; i < totalChunks; i++ {
		if err := ctx.Err(); err != nil {
			return relaySendResult{}, ctxErr(err)
		}
		want := chunkSize
		if rem := plainTotal - int64(i)*int64(chunkSize); rem < int64(want) {
			want = int(rem)
		}
		if _, err := io.ReadFull(seq, buf[:want]); err != nil {
			return relaySendResult{}, err
		}
		ct, hsum, err := cc.SealChunk(i, buf[:want])
		if err != nil {
			return relaySendResult{}, err
		}
		cts[i] = ct
		hashes[i] = hsum
		totalCT += int64(len(ct))
	}
	if totalCT > plan.effectiveMax+int64(wire.MaxControl)+int64(totalChunks)*relaycrypto.GCMTagSize {
		return relaySendResult{}, newErr(CodeLimitExceeded, "encrypted payload exceeds relay limit")
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
		return relaySendResult{}, err
	}

	// Fast path: 1-RTT single-chunk upload.
	if totalChunks == 1 {
		if err := cli.UploadSingle(ctx, plan.route.MailboxID, plan.route.DeliveryToken, env, cts[0]); err != nil {
			return relaySendResult{}, err
		}
		h.ReportProgress(plan.key, "", TransferProgress{BytesSent: plan.view.set.total, TotalBytes: plan.view.set.total, PartCount: len(plan.view.set.parts)})
		return relaySendResult{state: TargetSent, receiptStage: ReceiptStageRelayStored, route: RouteRelay}, nil
	}

	// Multi-chunk path: announce envelope then upload chunks with bounded concurrency,
	// seamlessly settling onto P2P mid-transfer if P2P recovers (Level 3, contract §5).
	if _, err := cli.CreateTransfer(ctx, plan.route.MailboxID, plan.route.DeliveryToken, env); err != nil {
		return relaySendResult{}, err
	}

	deleteOnAbort := func() {
		delCtx, cancel := context.WithTimeout(context.Background(), relayCleanupTimeout)
		_ = cli.DeleteTransferAsSender(delCtx, plan.route.MailboxID, plan.route.DeliveryToken, plan.clip.ID)
		cancel()
	}

	httpUploaded, _ := relaycrypto.NewBitmap(totalChunks)
	remoteHave, _ := relaycrypto.NewBitmap(totalChunks)
	var (
		stateMu          sync.Mutex
		maxReportedPlain int64
		usedHybrid       bool
		switches         int
	)
	routeOf := func() TransportRoute {
		if usedHybrid {
			return RouteHybrid
		}
		return RouteRelay
	}
	remainingLocked := func() ([]int, int64) {
		var idx []int
		var remBytes int64
		for i := 0; i < totalChunks; i++ {
			if !relaycrypto.HasBit(httpUploaded, i) && !relaycrypto.HasBit(remoteHave, i) {
				idx = append(idx, i)
				remBytes += int64(len(cts[i]))
			}
		}
		return idx, remBytes
	}
	reportProgress := func(p2pSent []byte) {
		stateMu.Lock()
		var doneCT int64
		for i := 0; i < totalChunks; i++ {
			if relaycrypto.HasBit(httpUploaded, i) || relaycrypto.HasBit(remoteHave, i) || relaycrypto.HasBit(p2pSent, i) {
				doneCT += int64(len(cts[i]))
			}
		}
		approxPlain := min((doneCT*plan.view.set.total)/max(totalCT, 1), plan.view.set.total)
		if approxPlain < maxReportedPlain {
			approxPlain = maxReportedPlain
		} else {
			maxReportedPlain = approxPlain
		}
		val := maxReportedPlain
		stateMu.Unlock()
		h.ReportProgress(plan.key, "", TransferProgress{
			BytesSent:  val,
			TotalBytes: plan.view.set.total,
			PartCount:  len(plan.view.set.parts),
		})
	}

	hb, _ := b.pb.(plugin.PeerRelayHandoffBridge)
	canHandoff := func(remCount int, remBytes int64) bool {
		if hb == nil || switches >= maxHandoffSwitches {
			return false
		}
		if remCount < handoffMinRemainingChunks || remBytes < handoffMinRemainingBytes {
			return false
		}
		connected, handoffCap, _ := hb.PeerHandoffCaps(plan.peerID)
		return connected && handoffCap
	}

	wakeCh, unwake := e.relay.watchPeerUp(plan.peerID)
	defer unwake()

	envJSON, err := json.Marshal(env)
	if err != nil {
		deleteOnAbort()
		return relaySendResult{}, err
	}

	for {
		if err := ctx.Err(); err != nil {
			deleteOnAbort()
			return relaySendResult{}, ctxErr(err)
		}
		stateMu.Lock()
		remIdx, remBytes := remainingLocked()
		uploadedCopy := append([]byte(nil), httpUploaded...)
		stateMu.Unlock()

		if len(remIdx) == 0 {
			return relaySendResult{state: TargetSent, receiptStage: ReceiptStageRelayStored, route: routeOf()}, nil
		}

		// Branch A: Attempt Level 3 P2P handoff settlement if peer is connected with CapRelayHandoff.
		if canHandoff(len(remIdx), remBytes) {
			ackCh, sessDone, cleanupAck, hasAck := hb.RegisterRelayClipAckWaiter(plan.peerID, plan.clip.ID)
			offerAck, oerr := hb.SendRelayHandoffOffer(ctx, plan.peerID, plugin.RelayHandoffOffer{
				TransferID:          plan.clip.ID,
				EnvelopeJSON:        envJSON,
				RelayUploadedBitmap: relaycrypto.EncodeBitmap(uploadedCopy),
			})
			if oerr != nil {
				cleanupAck()
				if ctx.Err() != nil {
					deleteOnAbort()
					return relaySendResult{}, ctxErr(ctx.Err())
				}
				// P2P flapped during offer; fall through to HTTP batch below.
			} else if !offerAck.Accept {
				cleanupAck()
				switch offerAck.Reason {
				case "discarded", "policy", "limit_exceeded", "permission_denied":
					deleteOnAbort()
					return relaySendResult{state: TargetDiscarded, receiptStage: ReceiptStageRemoteDiscarded, route: RouteHybrid}, nil
				}
				// Transient receiver rejection (e.g. unavailable/resource_exhausted): fall through to HTTP.
			} else {
				switches++
				usedHybrid = true
				h.ReportRoute(plan.key, RouteHybrid)

				if offerAck.HaveBitmap != "" {
					if haveBM, derr := relaycrypto.DecodeBitmap(offerAck.HaveBitmap, totalChunks); derr == nil {
						stateMu.Lock()
						for i := 0; i < totalChunks; i++ {
							if relaycrypto.HasBit(haveBM, i) {
								relaycrypto.SetBit(remoteHave, i)
							}
						}
						stateMu.Unlock()
					}
				}
				p2pSent, _ := relaycrypto.NewBitmap(totalChunks)
				reportProgress(p2pSent)

				var wantList []int
				if offerAck.WantP2PBitmap != "" {
					if wantBM, derr := relaycrypto.DecodeBitmap(offerAck.WantP2PBitmap, totalChunks); derr == nil {
						for i := 0; i < totalChunks; i++ {
							if relaycrypto.HasBit(wantBM, i) {
								wantList = append(wantList, i)
							}
						}
					}
				}

				checkEarlyAck := func() (done bool, res relaySendResult, rerr error) {
					if !hasAck {
						return false, relaySendResult{}, nil
					}
					select {
					case status := <-ackCh:
						switch status {
						case wire.ClipAckReceived:
							return true, relaySendResult{state: TargetSent, receiptStage: ReceiptStageRemoteReceived, route: RouteHybrid}, nil
						case wire.ClipAckDiscarded:
							deleteOnAbort()
							return true, relaySendResult{state: TargetDiscarded, receiptStage: ReceiptStageRemoteDiscarded, route: RouteHybrid}, nil
						default:
							deleteOnAbort()
							return true, relaySendResult{}, newErr(CodeUnavailable, "peer failed to receive handoff transfer")
						}
					default:
						return false, relaySendResult{}, nil
					}
				}

				p2pBroken := false
				for _, idx := range wantList {
					if done, res, rerr := checkEarlyAck(); done {
						cleanupAck()
						return res, rerr
					}
					if cerr := hb.SendRelayChunk(ctx, plan.peerID, plan.clip.ID, idx, cts[idx]); cerr != nil {
						if done, res, rerr := checkEarlyAck(); done {
							cleanupAck()
							return res, rerr
						}
						cleanupAck()
						if ctx.Err() != nil {
							deleteOnAbort()
							return relaySendResult{}, ctxErr(ctx.Err())
						}
						// Reverse fallback: P2P dropped mid-handoff; resume HTTP for all chunks not in (httpUploaded ∪ remoteHave).
						p2pBroken = true
						break
					}
					relaycrypto.SetBit(p2pSent, idx)
					reportProgress(p2pSent)
				}
				if p2pBroken {
					continue
				}

				// Defensive check: upload any chunk not covered by httpUploaded ∪ remoteHave ∪ p2pSent.
				var uncovered []int
				stateMu.Lock()
				for i := 0; i < totalChunks; i++ {
					if !relaycrypto.HasBit(httpUploaded, i) && !relaycrypto.HasBit(remoteHave, i) && !relaycrypto.HasBit(p2pSent, i) {
						uncovered = append(uncovered, i)
					}
				}
				stateMu.Unlock()
				for _, idx := range uncovered {
					if uerr := cli.UploadChunk(ctx, plan.route.MailboxID, plan.route.DeliveryToken, plan.clip.ID, idx, cts[idx]); uerr != nil {
						cleanupAck()
						deleteOnAbort()
						return relaySendResult{}, uerr
					}
					stateMu.Lock()
					relaycrypto.SetBit(httpUploaded, idx)
					stateMu.Unlock()
					reportProgress(p2pSent)
				}

				if !hasAck {
					cleanupAck()
					return relaySendResult{state: TargetSent, receiptStage: ReceiptStageLocalWritten, route: RouteHybrid}, nil
				}

				h.ReportProgress(plan.key, ReceiptStageLocalWritten, TransferProgress{
					BytesSent:  plan.view.set.total,
					TotalBytes: plan.view.set.total,
					PartCount:  len(plan.view.set.parts),
				})
				ackTimer := time.NewTimer(handoffClipAckTimeout)
				select {
				case status := <-ackCh:
					ackTimer.Stop()
					cleanupAck()
					switch status {
					case wire.ClipAckReceived:
						return relaySendResult{state: TargetSent, receiptStage: ReceiptStageRemoteReceived, route: RouteHybrid}, nil
					case wire.ClipAckDiscarded:
						deleteOnAbort()
						return relaySendResult{state: TargetDiscarded, receiptStage: ReceiptStageRemoteDiscarded, route: RouteHybrid}, nil
					default:
						deleteOnAbort()
						return relaySendResult{}, newErr(CodeUnavailable, "peer failed to commit handoff transfer")
					}
				case <-sessDone:
					// P2P dropped before clip_ack arrived; fall back to HTTP for unconfirmed chunks so zero bytes are lost.
					ackTimer.Stop()
					cleanupAck()
					continue
				case <-ctx.Done():
					ackTimer.Stop()
					cleanupAck()
					deleteOnAbort()
					return relaySendResult{}, ctxErr(ctx.Err())
				case <-ackTimer.C:
					cleanupAck()
					return relaySendResult{}, newErr(CodeTimeout, "timed out waiting for peer handoff confirmation")
				}
			}
		}

		// Branch B: Concurrent HTTP chunk upload batch, preemptible by P2P recovery.
		httpCtx, httpCancel := context.WithCancel(ctx)
		var handoffRequested atomic.Bool
		var monWG sync.WaitGroup
		if hb != nil && switches < maxHandoffSwitches {
			monWG.Add(1)
			go func() {
				defer monWG.Done()
				for {
					select {
					case <-httpCtx.Done():
						return
					case <-wakeCh:
						stateMu.Lock()
						rIdx, rBytes := remainingLocked()
						stateMu.Unlock()
						if canHandoff(len(rIdx), rBytes) {
							handoffRequested.Store(true)
							httpCancel()
							return
						}
					}
				}
			}()
		}

		tasks := make(chan int, len(remIdx))
		for _, idx := range remIdx {
			tasks <- idx
		}
		close(tasks)

		var (
			wg       sync.WaitGroup
			errMu    sync.Mutex
			firstErr error
		)
		workers := min(relayUploadConcurrency, len(remIdx))
		for w := 0; w < workers; w++ {
			wg.Add(1)
			go func() {
				defer wg.Done()
				for idx := range tasks {
					if httpCtx.Err() != nil {
						return
					}
					if uerr := cli.UploadChunk(httpCtx, plan.route.MailboxID, plan.route.DeliveryToken, plan.clip.ID, idx, cts[idx]); uerr != nil {
						if handoffRequested.Load() && ctx.Err() == nil {
							return
						}
						errMu.Lock()
						if firstErr == nil {
							firstErr = uerr
						}
						errMu.Unlock()
						httpCancel()
						return
					}
					stateMu.Lock()
					relaycrypto.SetBit(httpUploaded, idx)
					stateMu.Unlock()
					reportProgress(nil)
				}
			}()
		}
		wg.Wait()
		httpCancel()
		monWG.Wait()

		if firstErr != nil {
			deleteOnAbort()
			return relaySendResult{}, firstErr
		}
		if handoffRequested.Load() {
			continue
		}
		// Re-loop to verify all chunks are covered and return cleanly.
	}
}

// encodeJSONForTest keeps json import used cleanly if needed in future extensions.
var _ = json.Marshal
