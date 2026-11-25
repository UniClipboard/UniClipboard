package clipboardintegration

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"sync"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// ErrUncertain reports that a clipboard write was started but its service
// completion could not be confirmed. Callers must not inject paste input or
// automatically replay the write.
var ErrUncertain = errors.New("clipboard apply completion unconfirmed")

// ReleaseBoard is optionally implemented by native boards that can notify when
// clipboard selection ownership ends (for example mygo.ClipboardOptions.OnRelease).
type ReleaseBoard interface {
	Board
	WriteWithRelease(s clip.Snapshot, onRelease func()) error
}

// ChangeWatchBoard is optionally implemented by boards that expose event-driven
// clipboard change subscriptions without background polling timers.
type ChangeWatchBoard interface {
	WatchChanges(fn func()) (stop func())
}

type pendingPanelRelease struct {
	serviceID    string
	token        engine.ApplyToken
	currentPaths []string
}

// PanelClipboard executes Quick Panel clipboard writes on Windows and Linux
// directly in the panel process under unicd's bounded panel-apply protocol.
type PanelClipboard struct {
	profile string
	board   Board

	mu              sync.Mutex
	client          *api.Client
	heldService     string
	heldToken       engine.ApplyToken
	heldPaths       []string
	stopWatch       func()
	pendingReleases []pendingPanelRelease
	closed          bool

	// BeforeComplete is an optional test hook invoked after local write and
	// read-back verification but before sending /v1/desktop/panel-apply/complete.
	BeforeComplete func() error
}

func NewPanelClipboard(profile string, b Board) *PanelClipboard {
	return &PanelClipboard{profile: profile, board: b}
}

func (p *PanelClipboard) Apply(ctx context.Context, client *api.Client, id string, plain bool) (string, error) {
	if client == nil || p.board == nil || id == "" {
		return "", ErrClipboard
	}
	p.mu.Lock()
	p.client = client
	p.mu.Unlock()
	p.flushPendingReleases(client)

	var obsBefore *clip.Snapshot
	if before, err := p.board.Read(); err == nil {
		obsBefore = &before
	}

	var reqBytes [16]byte
	if _, err := rand.Read(reqBytes[:]); err != nil {
		return "", err
	}
	requestID := "panel-" + hex.EncodeToString(reqBytes[:])

	grant, err := client.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:        p.profile,
		RequestID:      requestID,
		HistoryID:      id,
		Plain:          plain,
		ObservedBefore: obsBefore,
	})
	if err != nil {
		if api.ReasonOf(err) == "plain" {
			return "", ErrPlain
		}
		return "", ErrClipboard
	}
	if grant.Replayed {
		back, rerr := p.board.Read()
		if rerr != nil {
			return "", ErrClipboard
		}
		mark := clipboardMark(back)
		if mark == "" {
			return "", ErrClipboard
		}
		return mark, nil
	}

	reportFailed := func(code string) {
		fctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_, _ = client.PanelApplyComplete(fctx, api.PanelApplyCompleteRequest{
			Profile:   p.profile,
			ServiceID: grant.ServiceID,
			RequestID: requestID,
			Token:     grant.Token,
			Outcome:   "failed",
			ErrorCode: code,
		})
	}

	var onRelease func()
	if grant.HasFiles {
		serviceID, token := grant.ServiceID, grant.Token
		onRelease = func() {
			go p.checkRelease(serviceID, token)
		}
	}

	if rb, ok := p.board.(ReleaseBoard); ok {
		err = rb.WriteWithRelease(grant.Snapshot, onRelease)
	} else {
		err = p.board.Write(grant.Snapshot)
	}
	if err != nil {
		reportFailed("write_failed")
		return "", ErrClipboard
	}

	back, rerr := p.board.Read()
	if rerr != nil || !sameWrittenSnapshot(grant.Snapshot, back) {
		reportFailed("conflict")
		return "", ErrClipboard
	}
	mark := clipboardMark(back)
	if mark == "" {
		reportFailed("conflict")
		return "", ErrClipboard
	}

	p.mu.Lock()
	hook := p.BeforeComplete
	p.mu.Unlock()
	if hook != nil {
		if herr := hook(); herr != nil {
			if grant.HasFiles {
				p.updateFileLease(grant)
			}
			return "", errors.Join(ErrUncertain, herr)
		}
	}

	reply, cerr := client.PanelApplyComplete(ctx, api.PanelApplyCompleteRequest{
		Profile:   p.profile,
		ServiceID: grant.ServiceID,
		RequestID: requestID,
		Token:     grant.Token,
		Outcome:   "applied",
		ReadBack:  &back,
	})
	if cerr != nil || !reply.Applied {
		// Uncertain write outcome: never inject paste input and never replay.
		// Retain file lease tracking if files were already written so later
		// clipboard changes or process exit can still release the service hold.
		if grant.HasFiles {
			p.updateFileLease(grant)
		}
		return "", ErrUncertain
	}

	p.updateFileLease(grant)
	return mark, nil
}

func (p *PanelClipboard) updateFileLease(grant api.PanelApplyGrant) {
	p.mu.Lock()
	if p.stopWatch != nil {
		p.stopWatch()
		p.stopWatch = nil
	}
	if !grant.HasFiles {
		p.heldService = ""
		p.heldToken = ""
		p.heldPaths = nil
		p.mu.Unlock()
		return
	}
	paths := make([]string, 0, len(grant.Snapshot.Files))
	for _, f := range grant.Snapshot.Files {
		paths = append(paths, f.Path)
	}
	p.heldService = grant.ServiceID
	p.heldToken = grant.Token
	p.heldPaths = paths
	serviceID, token := grant.ServiceID, grant.Token
	var cw ChangeWatchBoard
	if b, ok := p.board.(ChangeWatchBoard); ok {
		cw = b
	}
	p.mu.Unlock()

	if cw != nil {
		stop := cw.WatchChanges(func() {
			go p.checkRelease(serviceID, token)
		})
		p.mu.Lock()
		if p.heldService == serviceID && p.heldToken == token && !p.closed {
			p.stopWatch = stop
		} else if stop != nil {
			stop()
		}
		p.mu.Unlock()
	}
	// Reconcile once right after recording heldToken and installing the watcher
	// in case the clipboard was replaced before the watcher became active.
	go p.checkRelease(serviceID, token)
}

func (p *PanelClipboard) checkRelease(serviceID string, token engine.ApplyToken) {
	p.checkReleaseAttempt(serviceID, token, 0)
}

func (p *PanelClipboard) checkReleaseAttempt(serviceID string, token engine.ApplyToken, attempt int) {
	snap, readErr := p.board.Read()

	p.mu.Lock()
	if p.closed || p.heldService != serviceID || p.heldToken != token {
		p.mu.Unlock()
		return
	}
	if readErr != nil {
		// Transient clipboard read failure must not drop the managed-file hold
		// or watcher. Schedule a bounded retry; later clipboard events or Close
		// will also recheck.
		p.mu.Unlock()
		if attempt < 3 {
			time.AfterFunc(150*time.Millisecond, func() {
				p.checkReleaseAttempt(serviceID, token, attempt+1)
			})
		}
		return
	}
	if sameHeldPaths(snap, p.heldPaths) {
		// A clipboard manager or desktop host may take ownership while still
		// holding any of the protected file paths. Keep the managed file lease alive.
		p.mu.Unlock()
		return
	}
	stop := p.stopWatch
	p.stopWatch = nil
	p.heldService = ""
	p.heldToken = ""
	p.heldPaths = nil
	client := p.client
	p.mu.Unlock()

	if stop != nil {
		stop()
	}
	if client == nil {
		return
	}
	var currentPaths []string
	for _, f := range snap.Files {
		currentPaths = append(currentPaths, f.Path)
	}
	p.sendRelease(client, pendingPanelRelease{
		serviceID:    serviceID,
		token:        token,
		currentPaths: currentPaths,
	}, true)
}

func (p *PanelClipboard) sendRelease(client *api.Client, rel pendingPanelRelease, queueOnFail bool) bool {
	if client == nil || rel.token == "" {
		return true
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	err := client.PanelApplyRelease(ctx, api.PanelApplyReleaseRequest{
		Profile:      p.profile,
		ServiceID:    rel.serviceID,
		Token:        rel.token,
		CurrentPaths: rel.currentPaths,
	})
	if err == nil {
		return true
	}
	if queueOnFail {
		p.mu.Lock()
		if len(p.pendingReleases) < 16 {
			p.pendingReleases = append(p.pendingReleases, rel)
		}
		p.mu.Unlock()
	}
	return false
}

func (p *PanelClipboard) flushPendingReleases(client *api.Client) {
	if client == nil {
		return
	}
	p.mu.Lock()
	if len(p.pendingReleases) == 0 {
		p.mu.Unlock()
		return
	}
	pending := append([]pendingPanelRelease(nil), p.pendingReleases...)
	p.pendingReleases = nil
	p.mu.Unlock()

	var remaining []pendingPanelRelease
	for _, rel := range pending {
		if !p.sendRelease(client, rel, false) {
			remaining = append(remaining, rel)
		}
	}
	if len(remaining) > 0 {
		p.mu.Lock()
		p.pendingReleases = append(remaining, p.pendingReleases...)
		if len(p.pendingReleases) > 16 {
			p.pendingReleases = p.pendingReleases[:16]
		}
		p.mu.Unlock()
	}
}

func (p *PanelClipboard) Matches(mark string) bool {
	if mark == "" || p.board == nil {
		return false
	}
	s, err := p.board.Read()
	return err == nil && clipboardMark(s) == mark
}

// Close releases any held panel file lease, retries pending releases, and stops active change listeners.
func (p *PanelClipboard) Close() {
	p.mu.Lock()
	p.closed = true
	stop := p.stopWatch
	p.stopWatch = nil
	serviceID := p.heldService
	token := p.heldToken
	client := p.client
	p.heldService = ""
	p.heldToken = ""
	p.heldPaths = nil
	p.mu.Unlock()

	if stop != nil {
		stop()
	}
	p.flushPendingReleases(client)
	if token != "" && client != nil {
		p.sendRelease(client, pendingPanelRelease{
			serviceID:    serviceID,
			token:        token,
			currentPaths: nil,
		}, false)
	}
}
