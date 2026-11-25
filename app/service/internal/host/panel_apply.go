package host

import (
	"context"
	"errors"
	"io"
	"path/filepath"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/directory"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

var panelApplyTTL = 8 * time.Second

type panelApplyTx struct {
	serviceID       string
	requestID       string
	historyID       string
	contentID       engine.ContentID
	token           engine.ApplyToken
	lease           engine.LeaseID
	syncLease       engine.LeaseID
	epoch           string
	userRestore     bool
	hasFiles        bool
	filePaths       []string
	withObserver    bool
	observerSession string
	expiresAt       time.Time
	done            chan struct{}
}

type panelRestoreRecord struct {
	historyID string
	at        time.Time
	inFlight  bool
	done      chan struct{}
	result    engine.RestoreSyncResult
	err       error
}

func (a *adapterState) cancelPanelRestoreLocked(requestID string) {
	if r := a.panelRestores[requestID]; r != nil && r.inFlight {
		delete(a.panelRestores, requestID)
		if r.done != nil {
			close(r.done)
		}
	}
}

func (a *adapterState) retainPanelHeldLocked(tx *panelApplyTx) {
	oldHeld := a.panelHeldLease
	a.panelHeldLease = tx.lease
	a.panelHeldToken = tx.token
	a.panelHeldPaths = tx.filePaths
	a.panelHeldEpoch = a.obsEpoch
	a.panelHeldSeq = a.lastEngineSeq
	a.panelHeldSeen = false
	if oldHeld != "" && oldHeld != a.panelHeldLease {
		_ = a.n.e.ReleaseContent(context.Background(), oldHeld)
	}
}

func (a *adapterState) shouldSuppressPostPanelObserveLocked(lease engine.LeaseID) bool {
	if lease == "" {
		return true
	}
	cid := a.leaseContent[lease]
	if cid == "" {
		return false
	}
	return cid == a.lastPanelPostContent || cid == a.lastPanelPreContent
}

func (a *adapterState) reconcilePostHoldLocked() bool {
	if a.obsEpoch != "" && a.obsEpoch == a.panelHeldEpoch && a.lastEngineSeq > a.panelHeldSeq {
		return true
	}
	if a.baseline && a.obsEpoch != "" && a.obsEpoch != a.panelHeldEpoch {
		return true
	}
	return false
}

func (a *adapterState) expirePanelTxLocked(now time.Time) {
	if a.panelTx != nil && now.After(a.panelTx.expiresAt) {
		a.abortPanelTxLocked("timeout")
	}
}

func (a *adapterState) abortPanelTxLocked(errorCode string) {
	tx := a.panelTx
	if tx == nil {
		return
	}
	a.panelTx = nil
	a.cancelPanelRestoreLocked(tx.requestID)
	_ = a.n.e.ReportApplyResult(context.Background(), engine.ApplyResult{
		Token:     tx.token,
		Outcome:   engine.ApplyOutcomeFailed,
		ErrorCode: errorCode,
	})
	if tx.hasFiles && tx.lease != "" {
		// Uncertain outcome: the panel may already have written these managed
		// files to the OS clipboard before timing out or losing connection.
		// Retain the lease until clipboard change, release, reconciliation, or shutdown.
		a.retainPanelHeldLocked(tx)
	} else if tx.lease != "" {
		_ = a.n.e.ReleaseContent(context.Background(), tx.lease)
	}
	if tx.syncLease != "" {
		_ = a.n.e.ReleaseContent(context.Background(), tx.syncLease)
	}
	if a.session == "" {
		_ = a.n.e.UpdateHostCapabilities(context.Background(), engine.HostCapabilities{
			Observe: engine.AbilityUnavailable,
			Apply:   engine.AbilityUnavailable,
		})
	}
	close(tx.done)
	a.notifySlotLocked()
}

func (a *adapterState) releasePanelHeldLocked() {
	if a.panelHeldLease != "" {
		_ = a.n.e.ReleaseContent(context.Background(), a.panelHeldLease)
		a.panelHeldLease = ""
	}
	a.panelHeldToken = ""
	a.panelHeldPaths = nil
	a.panelHeldEpoch = ""
	a.panelHeldSeq = 0
	a.panelHeldSeen = false
}

// waitPanelClearLocked waits until any active panel transaction finishes or
// expires so background observer operations never race with an in-flight panel write.
// Caller holds a.mu; returns with a.mu held.
func (a *adapterState) waitPanelClearLocked(ctx context.Context) error {
	for {
		a.expirePanelTxLocked(time.Now())
		tx := a.panelTx
		if tx == nil {
			return nil
		}
		done := tx.done
		remaining := time.Until(tx.expiresAt)
		if remaining <= 0 {
			continue
		}
		a.mu.Unlock()
		timer := time.NewTimer(remaining)
		select {
		case <-done:
			timer.Stop()
		case <-timer.C:
		case <-ctx.Done():
			timer.Stop()
			a.mu.Lock()
			return adapterError(engine.CodeTimeout)
		}
		a.mu.Lock()
	}
}

// waitApplySlotLocked waits until both observer ticket and panelTx are idle.
// Caller holds a.mu; returns with a.mu held.
func (a *adapterState) waitApplySlotLocked(ctx context.Context) error {
	for {
		now := time.Now()
		if a.session != "" && now.After(a.until) {
			a.resetLocked()
		}
		a.expirePanelTxLocked(now)
		if a.ticket == "" && a.panelTx == nil {
			return nil
		}
		ch := a.slotChanged
		var waitLimit time.Duration
		if a.panelTx != nil {
			waitLimit = time.Until(a.panelTx.expiresAt)
		}
		a.mu.Unlock()
		var timer *time.Timer
		var timerCh <-chan time.Time
		if waitLimit > 0 {
			timer = time.NewTimer(waitLimit)
			timerCh = timer.C
		}
		select {
		case <-ch:
			if timer != nil {
				timer.Stop()
			}
		case <-timerCh:
		case <-ctx.Done():
			if timer != nil {
				timer.Stop()
			}
			a.mu.Lock()
			return &api.Error{Status: 409, Reason: "in_use", Message: "another clipboard application is in progress"}
		}
		a.mu.Lock()
	}
}

func (n *Node) checkPanelService(profile, serviceID string, requireServiceID bool) (servicelifecycle.Instance, error) {
	info := n.serviceInfo()
	if info.Retiring || info.Mode == servicelifecycle.ModeCLITemporary {
		return info, &api.Error{Status: 409, Reason: "unavailable", Message: "service is unavailable for panel clipboard application"}
	}
	if profile == "" {
		return info, &api.Error{Status: 400, Reason: "invalid_argument", Message: "profile is required"}
	}
	wantDir := canonicalDir(info.Directory)
	gotDir := canonicalDir(profile)
	if wantDir == "" || gotDir != wantDir {
		return info, &api.Error{Status: 409, Reason: "conflict", Message: "profile does not match service directory"}
	}
	if requireServiceID && serviceID == "" {
		return info, &api.Error{Status: 400, Reason: "invalid_argument", Message: "service_id is required"}
	}
	if serviceID != "" && serviceID != info.ID {
		return info, &api.Error{Status: 409, Reason: servicelifecycle.ReasonGenerationChanged, Message: "service generation changed"}
	}
	return info, nil
}

func canonicalDir(p string) string {
	if p == "" {
		return ""
	}
	abs, err := filepath.Abs(p)
	if err != nil {
		return ""
	}
	if resolved, err := filepath.EvalSymlinks(abs); err == nil {
		return filepath.Clean(resolved)
	}
	return filepath.Clean(abs)
}

func (n *Node) importPanelSnap(ctx context.Context, s *clip.Snapshot) (engine.LeaseID, engine.ContentID, error) {
	if s == nil || s.Empty() {
		return "", "", nil
	}
	in, err := contentInput(*s)
	if err != nil {
		return "", "", nil
	}
	h, err := n.e.ImportContent(ctx, in)
	if err != nil {
		var ee *engine.Error
		if errors.As(err, &ee) {
			switch ee.Code {
			case engine.CodeLimitExceeded, engine.CodeUnsupported, engine.CodeInvalidArgument, engine.CodeSourceChanged:
				return "", "", nil
			}
		}
		return "", "", err
	}
	return h.LeaseID, h.Content.ID, nil
}

func (n *Node) snapshotOfTicket(ctx context.Context, tk engine.ApplyTicket) (s clip.Snapshot, hasFiles bool, err error) {
	h, err := n.e.RetainContent(ctx, tk.ContentID)
	if err != nil {
		return s, false, err
	}
	defer func() { _ = n.e.ReleaseContent(context.Background(), h.LeaseID) }()
	for _, p := range h.Content.Parts {
		if p.MIME == directory.MIME {
			var paths []string
			refs, rerr := n.e.DirectoryReferences(ctx, tk.LeaseID)
			if rerr != nil {
				return s, false, rerr
			}
			for _, ref := range refs {
				path, perr := desktopFilePath(ref)
				if perr != nil {
					return s, false, perr
				}
				paths = append(paths, path)
			}
			if len(paths) == 0 {
				paths, err = n.e.DirectoryPaths(ctx, tk.LeaseID)
				if err != nil {
					return s, false, err
				}
			}
			for _, path := range paths {
				s.Files = append(s.Files, clip.File{Name: filepath.Base(path), Path: path})
			}
			return s, true, nil
		}
	}
	for _, p := range h.Content.Parts {
		if p.Kind != engine.PartFile {
			continue
		}
		var path string
		if p.Referenced {
			ref, ferr := n.e.FileReference(ctx, tk.LeaseID, p.ID)
			if ferr != nil {
				return s, false, ferr
			}
			path, err = desktopFilePath(ref)
		} else {
			path, err = n.e.LocalFilePath(ctx, tk.LeaseID, p.ID)
		}
		if err != nil {
			return s, false, err
		}
		s.Files = append(s.Files, clip.File{Name: p.Name, Path: path, Size: p.Size})
	}
	if len(s.Files) > 0 {
		return s, true, nil
	}
	for _, p := range h.Content.Parts {
		r, rerr := n.e.ReadContent(ctx, h.LeaseID, p.ID)
		if rerr != nil {
			return s, false, rerr
		}
		data, derr := io.ReadAll(r)
		_ = r.Close()
		if derr != nil {
			return s, false, derr
		}
		s.Parts = append(s.Parts, clip.Part{Format: p.MIME, Data: data})
	}
	return s, false, nil
}

func (a *adapterState) prunePanelRestoresLocked(now time.Time) {
	for k, r := range a.panelRestores {
		if !r.inFlight && now.Sub(r.at) >= 30*time.Minute {
			delete(a.panelRestores, k)
		}
	}
}

func (n *Node) PanelApplyBegin(ctx context.Context, req api.PanelApplyBeginRequest) (api.PanelApplyGrant, error) {
	var out api.PanelApplyGrant
	info, err := n.checkPanelService(req.Profile, req.ServiceID, false)
	if err != nil {
		return out, err
	}
	if req.RequestID == "" || len(req.RequestID) > 128 || req.HistoryID == "" {
		return out, &api.Error{Status: 400, Reason: "invalid_argument", Message: "request_id (1..128 bytes) and history_id are required"}
	}

	targetHistoryID := engine.HistoryID(req.HistoryID)
	if req.Plain {
		item, gerr := n.e.GetItem(ctx, targetHistoryID)
		if gerr != nil {
			return out, apiErr(gerr, false)
		}
		hasPlain := false
		for _, p := range item.Parts {
			if p.MIME == clip.Text {
				hasPlain = true
				break
			}
		}
		if !hasPlain {
			return out, &api.Error{Status: 400, Reason: "plain", Message: "history item has no plain-text representation"}
		}
		actionRes, aerr := n.e.RunAction(ctx, engine.RunActionRequest{
			RequestID: engine.RequestID("panel-plain-" + req.RequestID),
			Key:       "builtin.text/plain",
			HistoryID: targetHistoryID,
		})
		if aerr != nil {
			return out, apiErr(aerr, false)
		}
		targetHistoryID = actionRes.HistoryID
	}

	a := &n.adapter
	a.mu.Lock()
	defer a.mu.Unlock()

	// Re-check service status under lock in case service began retiring during RunAction.
	info, err = n.checkPanelService(req.Profile, req.ServiceID, false)
	if err != nil {
		return out, err
	}

	reservedRestore := false
	if !req.Plain {
		for {
			a.prunePanelRestoresLocked(time.Now())
			if old := a.panelRestores[req.RequestID]; old != nil {
				if old.historyID != req.HistoryID {
					return out, &api.Error{Status: 409, Reason: "conflict", Message: "request_id already names another history item"}
				}
				if old.inFlight {
					done := old.done
					a.mu.Unlock()
					select {
					case <-done:
					case <-ctx.Done():
						a.mu.Lock()
						return out, &api.Error{Status: 409, Reason: "in_use", Message: "restore request is already in progress"}
					}
					a.mu.Lock()
					info, err = n.checkPanelService(req.Profile, req.ServiceID, false)
					if err != nil {
						return out, err
					}
					continue
				}
				if old.err != nil {
					return out, old.err
				}
				res := old.result
				return api.PanelApplyGrant{
					ServiceID: info.ID,
					Replayed:  true,
					Restore:   &res,
				}, nil
			}
			if len(a.panelRestores) >= 1024 {
				return out, &api.Error{Status: 429, Reason: "resource_exhausted", Message: "restore request cache is full"}
			}
			a.panelRestores[req.RequestID] = &panelRestoreRecord{
				historyID: req.HistoryID,
				at:        time.Now(),
				inFlight:  true,
				done:      make(chan struct{}),
			}
			reservedRestore = true
			break
		}
	}
	releaseRestoreOnErr := func() {
		if reservedRestore {
			a.cancelPanelRestoreLocked(req.RequestID)
			reservedRestore = false
		}
	}

	if err := a.waitApplySlotLocked(ctx); err != nil {
		releaseRestoreOnErr()
		return out, err
	}
	// Re-check after waiting.
	info, err = n.checkPanelService(req.Profile, req.ServiceID, false)
	if err != nil {
		releaseRestoreOnErr()
		return out, err
	}

	withObserver := a.session != "" && a.baseline && a.obsEpoch != ""
	var ep string
	var expectedSeq uint64
	var observerSession string

	if withObserver {
		ep = a.obsEpoch
		expectedSeq = a.lastEngineSeq
		observerSession = a.session
	} else {
		if err := n.e.UpdateHostCapabilities(ctx, engine.HostCapabilities{
			WriteFormats: []string{engine.MimeText, engine.MimeHTML, engine.MimePNG},
			WriteFiles:   true,
			Observe:      engine.AbilityUnavailable,
			Apply:        engine.AbilityAvailable,
		}); err != nil {
			releaseRestoreOnErr()
			return out, apiErr(err, false)
		}
		restoreCaps := func() {
			if a.session == "" {
				_ = n.e.UpdateHostCapabilities(context.Background(), engine.HostCapabilities{
					Observe: engine.AbilityUnavailable,
					Apply:   engine.AbilityUnavailable,
				})
			}
		}
		beforeLease, beforeContent, ierr := n.importPanelSnap(ctx, req.ObservedBefore)
		if ierr != nil {
			restoreCaps()
			releaseRestoreOnErr()
			return out, apiErr(ierr, false)
		}
		ep = "panel-" + randomID()
		_, oerr := n.e.ObserveClipboard(ctx, engine.Observation{
			Epoch:    ep,
			Sequence: 1,
			Mode:     engine.ObserveBaseline,
			LeaseID:  beforeLease,
		})
		if beforeLease != "" {
			_ = n.e.ReleaseContent(context.Background(), beforeLease)
		}
		if oerr != nil {
			restoreCaps()
			releaseRestoreOnErr()
			return out, apiErr(oerr, false)
		}
		a.obsEpoch = ep
		a.lastEngineSeq = 1
		a.seqOffset = 0
		a.lastObservedContent = beforeContent
		expectedSeq = 1
	}

	tk, berr := n.e.BeginApplyHistory(ctx, engine.ApplyHistoryRequest{
		HistoryID:        targetHistoryID,
		UserRestore:      !req.Plain,
		ExpectedEpoch:    ep,
		ExpectedSequence: expectedSeq,
	})
	if berr != nil {
		if !withObserver && a.session == "" {
			_ = n.e.UpdateHostCapabilities(context.Background(), engine.HostCapabilities{
				Observe: engine.AbilityUnavailable,
				Apply:   engine.AbilityUnavailable,
			})
		}
		releaseRestoreOnErr()
		return out, apiErr(berr, false)
	}

	var syncLease engine.LeaseID
	if !req.Plain {
		held, rerr := n.e.RetainContent(ctx, tk.ContentID)
		if rerr != nil {
			_ = n.e.ReportApplyResult(context.Background(), engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeFailed, ErrorCode: "unavailable"})
			_ = n.e.ReleaseContent(context.Background(), tk.LeaseID)
			if !withObserver && a.session == "" {
				_ = n.e.UpdateHostCapabilities(context.Background(), engine.HostCapabilities{
					Observe: engine.AbilityUnavailable,
					Apply:   engine.AbilityUnavailable,
				})
			}
			releaseRestoreOnErr()
			return out, apiErr(rerr, false)
		}
		syncLease = held.LeaseID
	}

	snap, hasFiles, serr := n.snapshotOfTicket(ctx, tk)
	if serr != nil {
		_ = n.e.ReportApplyResult(context.Background(), engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeFailed, ErrorCode: "unavailable"})
		_ = n.e.ReleaseContent(context.Background(), tk.LeaseID)
		if syncLease != "" {
			_ = n.e.ReleaseContent(context.Background(), syncLease)
		}
		if !withObserver && a.session == "" {
			_ = n.e.UpdateHostCapabilities(context.Background(), engine.HostCapabilities{
				Observe: engine.AbilityUnavailable,
				Apply:   engine.AbilityUnavailable,
			})
		}
		releaseRestoreOnErr()
		return out, apiErr(serr, false)
	}

	var filePaths []string
	if hasFiles {
		filePaths = make([]string, 0, len(snap.Files))
		for _, f := range snap.Files {
			filePaths = append(filePaths, f.Path)
		}
	}

	a.panelTx = &panelApplyTx{
		serviceID:       info.ID,
		requestID:       req.RequestID,
		historyID:       req.HistoryID,
		contentID:       tk.ContentID,
		token:           tk.Token,
		lease:           tk.LeaseID,
		syncLease:       syncLease,
		epoch:           ep,
		userRestore:     !req.Plain,
		hasFiles:        hasFiles,
		filePaths:       filePaths,
		withObserver:    withObserver,
		observerSession: observerSession,
		expiresAt:       time.Now().Add(panelApplyTTL),
		done:            make(chan struct{}),
	}

	return api.PanelApplyGrant{
		ServiceID: info.ID,
		Epoch:     ep,
		Token:     tk.Token,
		Snapshot:  snap,
		HasFiles:  hasFiles,
	}, nil
}

func (n *Node) PanelApplyComplete(ctx context.Context, req api.PanelApplyCompleteRequest) (api.PanelApplyCompleteReply, error) {
	var out api.PanelApplyCompleteReply
	info, err := n.checkPanelService(req.Profile, req.ServiceID, true)
	if err != nil {
		return out, err
	}
	if req.RequestID == "" || req.Token == "" || (req.Outcome != "applied" && req.Outcome != "failed") {
		return out, &api.Error{Status: 400, Reason: "invalid_argument", Message: "invalid panel apply completion request"}
	}

	a := &n.adapter
	a.mu.Lock()
	a.expirePanelTxLocked(time.Now())
	tx := a.panelTx
	if tx == nil {
		a.mu.Unlock()
		return out, &api.Error{Status: 409, Reason: "resource_expired", Message: "panel apply transaction expired or not found"}
	}
	if tx.serviceID != info.ID || tx.requestID != req.RequestID || tx.token != req.Token {
		a.mu.Unlock()
		return out, &api.Error{Status: 409, Reason: "conflict", Message: "panel apply transaction mismatch"}
	}
	a.panelTx = nil

	finishTx := func() {
		if a.session == "" {
			_ = n.e.UpdateHostCapabilities(context.Background(), engine.HostCapabilities{
				Observe: engine.AbilityUnavailable,
				Apply:   engine.AbilityUnavailable,
			})
		}
		close(tx.done)
		a.notifySlotLocked()
	}

	if req.Outcome == "failed" {
		code := req.ErrorCode
		if code == "" {
			code = "write_failed"
		}
		a.cancelPanelRestoreLocked(tx.requestID)
		_ = n.e.ReportApplyResult(ctx, engine.ApplyResult{
			Token:     tx.token,
			Outcome:   engine.ApplyOutcomeFailed,
			ErrorCode: code,
		})
		if tx.lease != "" {
			_ = n.e.ReleaseContent(context.Background(), tx.lease)
		}
		if tx.syncLease != "" {
			_ = n.e.ReleaseContent(context.Background(), tx.syncLease)
		}
		finishTx()
		a.mu.Unlock()
		return api.PanelApplyCompleteReply{Applied: false}, nil
	}

	// Outcome == "applied": import read-back snapshot first before releasing any prior file lease.
	baseLease, baseContent, ierr := n.importPanelSnap(ctx, req.ReadBack)
	if ierr != nil {
		a.cancelPanelRestoreLocked(tx.requestID)
		_ = n.e.ReportApplyResult(context.Background(), engine.ApplyResult{
			Token:     tx.token,
			Outcome:   engine.ApplyOutcomeFailed,
			ErrorCode: "unavailable",
		})
		if tx.hasFiles && tx.lease != "" {
			a.retainPanelHeldLocked(tx)
		} else if tx.lease != "" {
			_ = n.e.ReleaseContent(context.Background(), tx.lease)
		}
		if tx.syncLease != "" {
			_ = n.e.ReleaseContent(context.Background(), tx.syncLease)
		}
		finishTx()
		a.mu.Unlock()
		return out, apiErr(ierr, false)
	}
	defer func() {
		if baseLease != "" {
			_ = n.e.ReleaseContent(context.Background(), baseLease)
		}
	}()

	nextSeq := a.lastEngineSeq + 1
	rerr := n.e.ReportApplyResult(ctx, engine.ApplyResult{
		Token:           tx.token,
		Outcome:         engine.ApplyOutcomeApplied,
		BaselineLeaseID: baseLease,
		Epoch:           tx.epoch,
		Sequence:        nextSeq,
	})
	if rerr != nil {
		a.cancelPanelRestoreLocked(tx.requestID)
		if tx.hasFiles && tx.lease != "" {
			a.retainPanelHeldLocked(tx)
		} else if tx.lease != "" {
			_ = n.e.ReleaseContent(context.Background(), tx.lease)
		}
		if tx.syncLease != "" {
			_ = n.e.ReleaseContent(context.Background(), tx.syncLease)
		}
		finishTx()
		a.mu.Unlock()
		return out, apiErr(rerr, false)
	}

	a.lastEngineSeq = nextSeq
	a.panelCommitSeq++
	postContent := baseContent
	if postContent == "" {
		postContent = tx.contentID
	}
	a.lastPanelPreContent = a.lastObservedContent
	a.lastPanelPostContent = postContent
	a.lastObservedContent = postContent
	if a.session != "" && a.session == tx.observerSession && a.obsEpoch == tx.epoch {
		a.seqOffset++
	}

	// Reconcile clipboard files before releasing any previously held panel file lease.
	var readBackPaths []string
	if req.ReadBack != nil {
		for _, f := range req.ReadBack.Files {
			readBackPaths = append(readBackPaths, f.Path)
		}
	}
	_ = n.e.ReconcileClipboardFiles(ctx, readBackPaths)
	if tx.hasFiles {
		a.retainPanelHeldLocked(tx)
	} else {
		a.releasePanelHeldLocked()
		if tx.lease != "" {
			_ = n.e.ReleaseContent(context.Background(), tx.lease)
		}
	}

	finishTx()
	a.mu.Unlock()

	out.Applied = true
	if tx.userRestore {
		defer func() {
			if tx.syncLease != "" {
				_ = n.e.ReleaseContent(context.Background(), tx.syncLease)
			}
		}()
		res := engine.RestoreSyncResult{LocalApplied: true, SyncDisposition: engine.SyncNotRequested}
		synced, serr := n.e.SyncRestoredHistory(ctx, tx.token, tx.syncLease)
		if serr != nil {
			res.SyncDisposition = engine.SyncFailed
			res.SyncError = &engine.Error{Code: engine.CodeUnavailable, Message: "local restore succeeded; sync could not start"}
		} else {
			res = synced
		}
		out.Restore = &res
		a.mu.Lock()
		a.prunePanelRestoresLocked(time.Now())
		if rec := a.panelRestores[tx.requestID]; rec != nil && rec.inFlight {
			rec.at = time.Now()
			rec.result = res
			rec.inFlight = false
			if rec.done != nil {
				close(rec.done)
			}
		} else {
			a.panelRestores[tx.requestID] = &panelRestoreRecord{
				historyID: tx.historyID,
				at:        time.Now(),
				result:    res,
			}
		}
		a.mu.Unlock()
	} else if tx.syncLease != "" {
		_ = n.e.ReleaseContent(context.Background(), tx.syncLease)
	}

	return out, nil
}

func (n *Node) PanelApplyRelease(ctx context.Context, req api.PanelApplyReleaseRequest) error {
	info, err := n.checkPanelService(req.Profile, req.ServiceID, true)
	if err != nil {
		return err
	}
	if req.Token == "" {
		return &api.Error{Status: 400, Reason: "invalid_argument", Message: "token is required"}
	}
	a := &n.adapter
	a.mu.Lock()
	defer a.mu.Unlock()
	if info.ID != req.ServiceID {
		return nil
	}
	if a.panelHeldLease == "" || a.panelHeldToken != req.Token {
		return nil
	}
	if hasAnyProtectedPath(req.CurrentPaths, a.panelHeldPaths) {
		return nil
	}
	_ = n.e.ReconcileClipboardFiles(ctx, req.CurrentPaths)
	a.releasePanelHeldLocked()
	return nil
}

func hasAnyProtectedPath(current, protected []string) bool {
	if len(current) == 0 || len(protected) == 0 {
		return false
	}
	set := make(map[string]struct{}, len(protected))
	for _, p := range protected {
		if p != "" {
			set[filepath.Clean(p)] = struct{}{}
		}
	}
	for _, p := range current {
		if p == "" {
			continue
		}
		if _, ok := set[filepath.Clean(p)]; ok {
			return true
		}
	}
	return false
}
