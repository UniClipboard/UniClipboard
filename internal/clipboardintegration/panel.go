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

// PanelClipboard executes Quick Panel clipboard writes on Windows and Linux
// directly in the panel process under unicd's bounded panel-apply protocol.
type PanelClipboard struct {
	profile string
	board   Board

	mu          sync.Mutex
	client      *api.Client
	heldService string
	heldToken   engine.ApplyToken
	heldPaths   []string
	stopWatch   func()

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
		if p.heldService == serviceID && p.heldToken == token {
			p.stopWatch = stop
		} else if stop != nil {
			stop()
		}
		p.mu.Unlock()
	}
}

func (p *PanelClipboard) checkRelease(serviceID string, token engine.ApplyToken) {
	snap, readErr := p.board.Read()

	p.mu.Lock()
	if p.heldService != serviceID || p.heldToken != token {
		p.mu.Unlock()
		return
	}
	if readErr == nil && sameHeldPaths(snap, p.heldPaths) {
		// A clipboard manager or desktop host may take ownership while still
		// holding the exact same file paths. Keep the managed file lease alive.
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
	if readErr == nil {
		for _, f := range snap.Files {
			currentPaths = append(currentPaths, f.Path)
		}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_ = client.PanelApplyRelease(ctx, api.PanelApplyReleaseRequest{
		Profile:      p.profile,
		ServiceID:    serviceID,
		Token:        token,
		CurrentPaths: currentPaths,
	})
}

func (p *PanelClipboard) Matches(mark string) bool {
	if mark == "" || p.board == nil {
		return false
	}
	s, err := p.board.Read()
	return err == nil && clipboardMark(s) == mark
}

// Close releases any held panel file lease and stops active change listeners.
func (p *PanelClipboard) Close() {
	p.mu.Lock()
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
	if token != "" && client != nil {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = client.PanelApplyRelease(ctx, api.PanelApplyReleaseRequest{
			Profile:      p.profile,
			ServiceID:    serviceID,
			Token:        token,
			CurrentPaths: nil,
		})
	}
}
